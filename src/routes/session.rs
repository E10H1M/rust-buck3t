// src/routes/session.rs
use actix_web::{web, HttpResponse, Result};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tokio::fs;
use std::path::PathBuf;

use crate::AppState;
use crate::consts::{Config, AuthMode};

pub(crate) fn init(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/auth")
            .route("/signup", web::post().to(signup))
            .route("/login",  web::post().to(login))
            .route("/logout", web::post().to(logout)),
    );
}

/* ---------- storage (dev-only, JSON file) ---------- */

#[derive(Serialize, Deserialize, Clone)]
struct StoredUser {
    username: String,
    // NOTE: dev-only — plaintext to keep deps minimal.
    // Replace with argon2/bcrypt before prod.
    password: String,
}

fn users_path() -> PathBuf {
    // Keep users out of the bucket. Override with AUTH_USER_DB if you like.
    // Defaults to ./auth/users.json
    let p = std::env::var("AUTH_USER_DB").unwrap_or_else(|_| "./auth/users.json".into());
    PathBuf::from(p)
}

async fn load_users(path: &PathBuf) -> Result<Vec<StoredUser>> {
    match fs::read(path).await {
        Ok(bytes) => {
            let users: Vec<StoredUser> = serde_json::from_slice(&bytes)
                .map_err(actix_web::error::ErrorInternalServerError)?;
            Ok(users)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(actix_web::error::ErrorInternalServerError(e)),
    }
}

async fn save_users(path: &PathBuf, users: &[StoredUser]) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(users)
        .map_err(actix_web::error::ErrorInternalServerError)?;

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)
            .await
            .map_err(actix_web::error::ErrorInternalServerError)?;
    }

    fs::write(path, bytes).await
        .map_err(actix_web::error::ErrorInternalServerError)?;
    Ok(())
}

/* ---------- requests / responses ---------- */

#[derive(Deserialize)]
struct SignupReq {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
    /// Optional: space-delimited scopes to request (default: all configured)
    scope: Option<String>,
    /// Optional: token TTL seconds (default 3600)
    ttl_secs: Option<u64>,
}

#[derive(Serialize)]
struct TokenResp {
    access_token: String,
    token_type: String, // "Bearer"
    expires_in: u64,
}

/* ---------- JWT claims ---------- */

#[derive(Serialize)]
struct Claims {
    sub: String,
    scope: String,              // space-delimited scopes
    exp: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aud: Option<String>,
}

/* ---------- handlers ---------- */

async fn signup(
    _state: web::Data<AppState>, // unused here now
    req: web::Json<SignupReq>,
) -> Result<HttpResponse> {
    let path = users_path();
    let mut users = load_users(&path).await?;

    if users.iter().any(|u| u.username == req.username) {
        return Err(actix_web::error::ErrorConflict("username already exists"));
    }

    users.push(StoredUser {
        username: req.username.clone(),
        password: req.password.clone(),
    });

    save_users(&path, &users).await?;
    Ok(HttpResponse::Created().finish())
}

async fn login(
    _state: web::Data<AppState>, // not needed for user storage anymore
    cfg: web::Data<Config>,
    req: web::Json<LoginReq>,
) -> Result<HttpResponse> {
    if !matches!(cfg.auth_mode, AuthMode::JwtHs256) {
        return Err(actix_web::error::ErrorBadRequest("login available only in HS256 mode"));
    }
    let secret = cfg.jwt_hs_secret.as_ref()
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("JWT_HS_SECRET not set"))?
        .clone();

    // verify credentials
    let path = users_path();
    let users = load_users(&path).await?;
    let Some(user) = users.into_iter().find(|u| u.username == req.username) else {
        return Err(actix_web::error::ErrorUnauthorized("invalid credentials"));
    };
    if user.password != req.password {
        return Err(actix_web::error::ErrorUnauthorized("invalid credentials"));
    }

    // scopes: requested or default to the configured sets
    let scope = req.scope.clone().unwrap_or_else(|| {
        let mut s = Vec::new();
        if !cfg.jwt_scopes_write.is_empty() { s.extend(cfg.jwt_scopes_write.clone()); }
        if !cfg.jwt_scopes_read.is_empty()  { s.extend(cfg.jwt_scopes_read.clone()); }
        if !cfg.jwt_scopes_list.is_empty()  { s.extend(cfg.jwt_scopes_list.clone()); }
        if s.is_empty() {
            "obj:write obj:read obj:list".to_string()
        } else {
            s.sort();
            s.dedup();
            s.join(" ")
        }
    });

    // NEW: clamp requested TTL to a server-side max (default 15 min)
    let ttl = req.ttl_secs.unwrap_or(900).min(cfg.auth_max_ttl_secs);



    let exp = (std::time::SystemTime::now()
        + std::time::Duration::from_secs(ttl))
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as usize;

    let iss = Some(format!("http://{}:{}", cfg.host, cfg.port));
    let aud = cfg.jwt_audience.clone();

    let mut header = Header::new(Algorithm::HS256);
    header.typ = Some("JWT".into());

    let claims = Claims { sub: user.username, scope, exp, iss, aud };

    let token = encode(&header, &claims, &EncodingKey::from_secret(secret.as_bytes()))
        .map_err(actix_web::error::ErrorInternalServerError)?;

    Ok(HttpResponse::Ok().json(TokenResp {
        access_token: token,
        token_type: "Bearer".into(),
        expires_in: ttl,
    }))
}

async fn logout() -> Result<HttpResponse> {
    // Stateless: client should delete token; server doesn't track sessions.
    Ok(HttpResponse::NoContent().finish())
}
