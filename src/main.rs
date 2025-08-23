// // src/main.rs
mod routes;
mod consts;

use actix_web::{web, App, HttpServer};
use std::path::PathBuf;
use crate::consts::Config;

#[derive(Clone)]
pub(crate) struct AppState {
    pub root: PathBuf,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let cfg = Config::from_env();

    tokio::fs::create_dir_all(&cfg.root_dir).await?;
    let state = AppState { root: PathBuf::from(&cfg.root_dir) };

    if let Some(limit) = cfg.max_upload_bytes {
        println!("📦 MAX_UPLOAD_BYTES = {} bytes", limit);
    } else {
        println!("📦 MAX_UPLOAD_BYTES not set (no upload size limit)");
    }

    println!("📂 RUST_BUCKET_DIR = {}", cfg.root_dir);

    println!(
        "🚀 rust-buck3t on http://{}:{}  (root = {})",
        cfg.host,
        cfg.port,
        state.root.display()
    );

    // clone cfg for closure use
    let cfg_clone = cfg.clone();

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .app_data(web::Data::new(cfg_clone.clone()))
            .configure(routes::health::init)
            .configure(routes::objects::init)
    })
    .bind((cfg.host.as_str(), cfg.port))?   // use original cfg here
    .run()
    .await
}