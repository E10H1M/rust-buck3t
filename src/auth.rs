// src/auth.rs
use actix_web::{
    dev::Payload,
    error::{ErrorForbidden, ErrorInternalServerError, ErrorUnauthorized},
    http::header,
    FromRequest, HttpRequest,
};
use futures_util::future::{ready, Ready};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde_json::Value;

use crate::consts::{AuthMode, Config};

/// Minimal authenticated user we might want later
#[derive(Clone, Debug)]
pub struct AuthUser {
    pub sub: Option<String>,
    pub scopes: Vec<String>,
    pub iss: Option<String>,
    pub aud: Vec<String>,
}

/// Require write scopes (PUT/DELETE)
pub struct NeedWrite(pub AuthUser);
/// Require read scopes (GET/HEAD)
pub struct NeedRead(pub AuthUser);
/// Require list scopes (list endpoints)
pub struct NeedList(pub AuthUser);

// ---------- Extractor impls ----------

impl FromRequest for NeedWrite {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;
    fn from_request(req: &HttpRequest, _pl: &mut Payload) -> Self::Future {
        ready(auth_gate(req, RouteClass::Write).map(NeedWrite))
    }
}
impl FromRequest for NeedRead {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;
    fn from_request(req: &HttpRequest, _pl: &mut Payload) -> Self::Future {
        ready(auth_gate(req, RouteClass::Read).map(NeedRead))
    }
}
impl FromRequest for NeedList {
    type Error = actix_web::Error;
    type Future = Ready<Result<Self, Self::Error>>;
    fn from_request(req: &HttpRequest, _pl: &mut Payload) -> Self::Future {
        ready(auth_gate(req, RouteClass::List).map(NeedList))
    }
}


// ---------- Core gate ----------

#[derive(Copy, Clone)]
enum RouteClass {
    Write,
    Read,
    List,
}

fn auth_gate(req: &HttpRequest, class: RouteClass) -> Result<AuthUser, actix_web::Error> {
    use actix_web::web::Data;
    use std::ops::Deref;

    let cfg = req
        .app_data::<Data<Config>>()
        .ok_or_else(|| ErrorInternalServerError("Config not found"))?
        .deref()
        .clone();

    // global off → allow
    if matches!(cfg.auth_mode, AuthMode::Off) {
        return Ok(AuthUser { sub: None, scopes: vec![], iss: None, aud: vec![] });
    }
    // class not protected → allow
    let class_protected = match class {
        RouteClass::Write => cfg.auth_write,
        RouteClass::Read  => cfg.auth_read,
        RouteClass::List  => cfg.auth_list,
    };
    if !class_protected {
        return Ok(AuthUser { sub: None, scopes: vec![], iss: None, aud: vec![] });
    }

    // bearer
    let token = bearer_token(req).map_err(|_| ErrorUnauthorized("missing or invalid Authorization header"))?;

    // verify by mode
    let user = match cfg.auth_mode {
        AuthMode::JwtHs256 => verify_hs256(&cfg, &token)?,
        AuthMode::JwtRs256 => return Err(ErrorInternalServerError("RS256 verifier not implemented yet")),
        AuthMode::Off => unreachable!(),
    };

    // scope check
    let required = match class {
        RouteClass::Write => &cfg.jwt_scopes_write,
        RouteClass::Read  => &cfg.jwt_scopes_read,
        RouteClass::List  => &cfg.jwt_scopes_list,
    };
    if !require_any_scope(required, &user.scopes) {
        return Err(ErrorForbidden("insufficient scope"));
    }

    Ok(user)
}


// ---------- Helpers ----------

/// Pulls the Bearer token from Authorization header
fn bearer_token(req: &HttpRequest) -> Result<String, ()> {
    let val = req.headers().get(header::AUTHORIZATION).ok_or(())?;
    let s = val.to_str().map_err(|_| ())?;
    const BEARER: &str = "Bearer ";
    if let Some(rest) = s.strip_prefix(BEARER) {
        Ok(rest.trim().to_string())
    } else {
        Err(())
    }
}

/// HS256 verification path
fn verify_hs256(cfg: &Config, token: &str) -> Result<AuthUser, actix_web::Error> {
    let secret = cfg
        .jwt_hs_secret
        .as_ref()
        .ok_or_else(|| ErrorInternalServerError("JWT_HS_SECRET not set"))?;

    let mut validation = Validation::new(Algorithm::HS256);
    // Enforce exp
    validation.validate_exp = true;
    // Pin algorithm
    validation.algorithms = vec![Algorithm::HS256];

    // jsonwebtoken's built-in aud/iss is finicky across versions; do explicit checks below.
    let data = decode::<Value>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| ErrorUnauthorized("invalid token"))?;

    let claims = data.claims;


    // Explicit exp enforcement (required)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ErrorUnauthorized("clock error"))?
        .as_secs();

    let exp = claims
        .get("exp")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| ErrorUnauthorized("exp missing"))?;

    if now >= exp {
        return Err(ErrorUnauthorized("token expired"));
    }    

    // iss allow-list (if configured)
    if !cfg.jwt_issuers.is_empty() {
        let iss = claims.get("iss").and_then(|v| v.as_str()).ok_or_else(|| ErrorUnauthorized("iss missing"))?;
        if !cfg.jwt_issuers.iter().any(|a| a == iss) {
            return Err(ErrorUnauthorized("issuer not allowed"));
        }
    }

    // audience (if configured)
    if let Some(expected_aud) = &cfg.jwt_audience {
        if !aud_matches(expected_aud, &claims) {
            return Err(ErrorUnauthorized("audience mismatch"));
        }
    }

    // scopes
    let scopes = scopes_from_claims(&claims);

    let sub = claims.get("sub").and_then(|v| v.as_str()).map(|s| s.to_string());
    let iss = claims.get("iss").and_then(|v| v.as_str()).map(|s| s.to_string());
    let aud = aud_values(&claims);

    Ok(AuthUser { sub, scopes, iss, aud })
}

/// Parse scopes from `scope` (space-delimited) or `scopes` (array) or `scp` (space-delimited).
fn scopes_from_claims(claims: &Value) -> Vec<String> {
    if let Some(s) = claims.get("scope").and_then(|v| v.as_str()) {
        return s.split_whitespace().map(|x| x.to_string()).collect();
    }
    if let Some(arr) = claims.get("scopes").and_then(|v| v.as_array()) {
        return arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect();
    }
    if let Some(s) = claims.get("scp").and_then(|v| v.as_str()) {
        return s.split_whitespace().map(|x| x.to_string()).collect();
    }
    Vec::new()
}

/// require any overlap between configured route scopes and token scopes.
/// If `required` is empty, allow (treat as not needed).
fn require_any_scope(required: &[String], token_scopes: &[String]) -> bool {
    if required.is_empty() {
        return true;
    }
    token_scopes.iter().any(|s| required.iter().any(|r| r == s))
}

/// Returns true if claims.aud matches expected (string or array)
fn aud_matches(expected: &str, claims: &Value) -> bool {
    match claims.get("aud") {
        Some(Value::String(s)) => s == expected,
        Some(Value::Array(arr)) => arr.iter().any(|v| v.as_str() == Some(expected)),
        _ => false,
    }
}

/// Collect aud into vec for AuthUser (string or array)
fn aud_values(claims: &Value) -> Vec<String> {
    match claims.get("aud") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str()).map(|s| s.to_string()).collect(),
        _ => vec![],
    }
}
