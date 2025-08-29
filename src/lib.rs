// src/lib.rs

use actix_web::{
    web, App,
    dev::{ServiceRequest, ServiceResponse},
    body::MessageBody,
    Error,
};
pub mod consts; 
pub mod auth;
mod routes;
use std::path::PathBuf;

#[derive(Clone)]
pub struct AppState {
    pub root: PathBuf,
}

pub fn app(
    state: AppState,
    cfg: consts::Config,
) -> App<
    impl actix_service::ServiceFactory<
        ServiceRequest,
        Response = ServiceResponse<impl MessageBody>,
        Config = (),
        InitError = (),
        Error = Error,
    >,
> {
    App::new()
        .app_data(web::Data::new(state))
        .app_data(web::Data::new(cfg))
        .configure(routes::health::init)
        .configure(routes::objects::init)
        .configure(routes::session::init)
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, http};

    #[actix_web::test]
    async fn app_builds_and_healthz_works() {
        let state = AppState { root: PathBuf::from("/tmp") };
        let cfg = consts::Config::from_env();
        let app = test::init_service(app(state, cfg)).await;

        let req = test::TestRequest::get().uri("/healthz").to_request();
        let resp = test::call_service(&app, req).await;

        assert_eq!(resp.status(), http::StatusCode::OK);
    }
}
