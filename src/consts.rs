// src/consts.rs

use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub root_dir: String,
    pub max_upload_bytes: Option<u64>,
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

        Self {
            host,
            port,
            root_dir,
            max_upload_bytes,
        }
    }
}

// static constants
pub(crate) const PATH_HEALTHZ: &str = "healthz";
pub(crate) const PATH_OBJECTS: &str = "objects";
