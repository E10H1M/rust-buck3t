// src/consts.rs

use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub root_dir: String,
    pub max_upload_bytes: Option<u64>,
    pub auth_max_ttl_secs: u64,

    // --- Auth config (config-only in this step) ---
    pub auth_mode: AuthMode,                 // "jwt_rs256" (default), "jwt_hs256", "off"
    pub auth_write: bool,                    // protect PUT/DELETE (default true)
    pub auth_read: bool,                     // protect GET/HEAD (default false)
    pub auth_list: bool,                     // protect listing (default false)
    pub jwt_scopes_write: Vec<String>,       // default ["obj:write"]
    pub jwt_scopes_read: Vec<String>,        // default ["obj:read"]
    pub jwt_scopes_list: Vec<String>,        // default ["obj:list"]
    pub jwt_audience: Option<String>,        // optional
    // RS256
    pub jwt_issuers: Vec<String>,            // CSV allow-list
    pub jwks_urls: Vec<String>,              // CSV optional explicit URLs
    pub jwks_ttl_secs: u64,                  // default 300
    // HS256
    pub jwt_hs_secret: Option<String>,       // required only in jwt_hs256 mode
    // Built-in IdP
    pub idp_embed: bool,                     // enable internal issuer (dev)
    pub idp_key_dir: String,                 // default "./keys"
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthMode {
    JwtRs256,
    JwtHs256,
    Off,
}

impl Config {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();

        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into());
        let port = env::var("PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(8080);

        let root_dir = env::var("RUST_BUCKET_DIR").unwrap_or_else(|_| "data".into());

        let max_upload_bytes = env::var("MAX_UPLOAD_BYTES")
            .ok()
            .and_then(|s| s.parse::<u64>().ok());

        let auth_max_ttl_secs = env::var("AUTH_MAX_TTL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(900);

        // --- Auth envs (config only; not enforced yet) ---
        let auth_mode = parse_auth_mode(&env::var("AUTH_MODE").unwrap_or_else(|_| "jwt_rs256".into()));
        let auth_write = parse_bool(env::var("AUTH_WRITE").ok()).unwrap_or(true);
        let auth_read  = parse_bool(env::var("AUTH_READ").ok()).unwrap_or(false);
        let auth_list  = parse_bool(env::var("AUTH_LIST").ok()).unwrap_or(false);

        let jwt_scopes_write = parse_csv(env::var("JWT_SCOPES_WRITE").ok()).unwrap_or_else(|| vec!["obj:write".into()]);
        let jwt_scopes_read  = parse_csv(env::var("JWT_SCOPES_READ").ok()).unwrap_or_else(|| vec!["obj:read".into()]);
        let jwt_scopes_list  = parse_csv(env::var("JWT_SCOPES_LIST").ok()).unwrap_or_else(|| vec!["obj:list".into()]);

        let jwt_audience = env::var("JWT_AUDIENCE").ok().filter(|s| !s.trim().is_empty());

        let jwt_issuers = parse_csv(env::var("JWT_ISSUERS").ok()).unwrap_or_default();
        let jwks_urls   = parse_csv(env::var("JWKS_URLS").ok()).unwrap_or_default();
        let jwks_ttl_secs = env::var("JWKS_TTL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(300);

        let jwt_hs_secret = env::var("JWT_HS_SECRET").ok().filter(|s| !s.trim().is_empty());

        let idp_embed = parse_bool(env::var("IDP_EMBED").ok()).unwrap_or(false);
        let idp_key_dir = env::var("IDP_KEY_DIR").unwrap_or_else(|_| "./keys".into());

        Self {
            host,
            port,
            root_dir,
            max_upload_bytes,
            auth_max_ttl_secs,
            auth_mode,
            auth_write,
            auth_read,
            auth_list,
            jwt_scopes_write,
            jwt_scopes_read,
            jwt_scopes_list,
            jwt_audience,
            jwt_issuers,
            jwks_urls,
            jwks_ttl_secs,
            jwt_hs_secret,
            idp_embed,
            idp_key_dir,
        }
    }

    /// Prints an auth config banner and (importantly) reads scope fields,
    /// so the library target doesn’t warn about them being unused.
    pub fn log_auth_banner(&self, host: &str, port: u16) {
        let mode_str = match self.auth_mode {
            AuthMode::JwtRs256 => "jwt_rs256",
            AuthMode::JwtHs256 => "jwt_hs256",
            AuthMode::Off => "off",
        };
        println!("🔐 AUTH_MODE = {}", mode_str);
        println!(
            "   • protected: write={} read={} list={}",
            self.auth_write, self.auth_read, self.auth_list
        );
        println!("   • scopes:");
        println!("     - write: {:?}", self.jwt_scopes_write);
        println!("     - read : {:?}", self.jwt_scopes_read);
        println!("     - list : {:?}", self.jwt_scopes_list);
        if let Some(aud) = &self.jwt_audience {
            println!("   • audience: {}", aud);
        }
        if !self.jwt_issuers.is_empty() {
            println!("   • issuers: {}", self.jwt_issuers.join(", "));
        }
        if !self.jwks_urls.is_empty() {
            println!("   • jwks_urls: {}", self.jwks_urls.join(", "));
        }
        println!("   • jwks_ttl_secs: {}", self.jwks_ttl_secs);
        if matches!(self.auth_mode, AuthMode::JwtHs256) && self.jwt_hs_secret.is_none() {
            eprintln!("⚠️  AUTH_MODE=jwt_hs256 but JWT_HS_SECRET is not set");
        }
        if matches!(self.auth_mode, AuthMode::JwtRs256) && self.jwt_issuers.is_empty() && !self.idp_embed {
            eprintln!("⚠️  AUTH_MODE=jwt_rs256 but JWT_ISSUERS is empty and IDP_EMBED=0; no issuers are permitted");
        }
        if self.idp_embed {
            println!(
                "🪪 Built-in IdP enabled (dev):\n   • JWKS: /{}\n   • Token mint: /{}\n   • Key dir: {}",
                PATH_JWKS, PATH_IDP_TOKEN, self.idp_key_dir
            );
            println!("   • Suggested iss: http://{}:{}", host, port);
        }
    }
}

// static constants
pub(crate) const PATH_HEALTHZ: &str = "healthz";
pub(crate) const PATH_OBJECTS: &str = "objects";
// Built-in IdP/JWKS endpoints (used in a later step)
pub(crate) const PATH_JWKS: &str = ".well-known/jwks.json";
pub(crate) const PATH_IDP_TOKEN: &str = "idp/token";

// ---- helpers ----
fn parse_csv(val: Option<String>) -> Option<Vec<String>> {
    val.map(|s| {
        s.split(',')
            .map(|x| x.trim())
            .filter(|x| !x.is_empty())
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
    })
}

fn parse_bool(val: Option<String>) -> Option<bool> {
    val.map(|s| {
        let t = s.trim().to_ascii_lowercase();
        matches!(t.as_str(), "1" | "true" | "yes" | "on")
    })
}

fn parse_auth_mode(s: &str) -> AuthMode {
    match s.trim().to_ascii_lowercase().as_str() {
        "jwt_hs256" => AuthMode::JwtHs256,
        "off" => AuthMode::Off,
        _ => AuthMode::JwtRs256,
    }
}
