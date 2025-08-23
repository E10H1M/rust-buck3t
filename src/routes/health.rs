use actix_web::{web, HttpResponse};
use crate::consts::PATH_HEALTHZ;

pub(crate) fn init(cfg: &mut web::ServiceConfig) {
    cfg.route(format!("/{}", PATH_HEALTHZ).as_str(), web::get().to(healthz));
}

async fn healthz() -> HttpResponse {
    println!("→ /{} endpoint hit", PATH_HEALTHZ);
    HttpResponse::Ok().body("ok")
}
