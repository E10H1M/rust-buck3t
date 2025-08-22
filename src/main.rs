// src/main.rs
mod routes;

use actix_web::{web, App, HttpServer};
use std::path::PathBuf;
use std::env;
use std::env::VarError;

#[derive(Clone)]
pub(crate) struct AppState {
    pub root: PathBuf,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Root directory for objects (defaults to ./data)
    let root = match env::var("RUST_BUCKET_DIR") {
        Ok(v) if !v.is_empty() => v,
        Ok(_) | Err(VarError::NotPresent) => {
            eprintln!("RUST_BUCKET_DIR not set or empty; defaulting to ./data");
            "data".into()
        }
        Err(VarError::NotUnicode(_)) => {
            eprintln!("RUST_BUCKET_DIR is not valid UTF-8; defaulting to ./data");
            "data".into()
        }
    };

    tokio::fs::create_dir_all(&root).await?;

    let state = AppState { root: PathBuf::from(root) };

    println!(
        "🚀 rust-buck3t on http://0.0.0.0:8080  (root = {})",
        state.root.display()
    );

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            // wire your route modules
            .configure(routes::health::init)
            .configure(routes::objects::init)
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
