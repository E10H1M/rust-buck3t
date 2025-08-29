// src/main.rs
use actix_web::HttpServer;
use std::path::PathBuf;

use rust_buck3t::consts::Config;
use rust_buck3t::{app, AppState};

fn banner(cfg: &Config, state_root: &PathBuf) {
    if let Some(limit) = cfg.max_upload_bytes {
        println!("📦 MAX_UPLOAD_BYTES = {} bytes", limit);
    } else {
        println!("📦 MAX_UPLOAD_BYTES not set (no upload size limit)");
    }
    println!("📂 RUST_BUCKET_DIR = {}", cfg.root_dir);
    println!("   • auth_max_ttl_secs: {}s", cfg.auth_max_ttl_secs);
    println!(
        "🚀 rust-buck3t on http://{}:{}  (root = {})",
        cfg.host,
        cfg.port,
        state_root.display()
    );
    cfg.log_auth_banner(&cfg.host, cfg.port);
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let cfg = Config::from_env();

    tokio::fs::create_dir_all(&cfg.root_dir).await?;
    let state = AppState { root: PathBuf::from(&cfg.root_dir) };

    banner(&cfg, &state.root);

    // prepare separate values for the closure and for bind()
    let cfg_for_server = cfg.clone();
    let state_for_server = state.clone();
    let bind_host = cfg.host.clone();
    let bind_port = cfg.port;

    HttpServer::new(move || {
        // use the cloned copies inside the closure
        app(state_for_server.clone(), cfg_for_server.clone())
    })
    .bind((bind_host.as_str(), bind_port))?
    .run()
    .await
}
