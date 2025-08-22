// routes/health.rs

use actix_web::{web, HttpResponse};

pub(crate) fn init(cfg: &mut web::ServiceConfig) {
    cfg.route("/healthz", web::get().to(healthz));
}

async fn healthz() -> HttpResponse {
    HttpResponse::Ok().body("ok")
}
