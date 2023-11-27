use crate::{config::Config, state::RouterState};
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, put},
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use std::fs::OpenOptions;
use tower_http::validate_request::ValidateRequestHeaderLayer;
use tracing_subscriber::prelude::*;

pub mod config;
pub mod handler;
pub mod state;
pub mod utility;

pub const EXTERNEL_ID: &str = "externalUserId";
pub const API: &str = "api";
pub const CODE: &str = "code";
pub const CODE_LENGTH: usize = 64;
pub const CONFIG_FILE_NAME: &str = "config.yaml";
pub const BODY_LIMIT: usize = 512 * 1024 * 1024;

/// 1. redirect service
/// 2. upload redirect table (id (str), url (str), params (dict))
/// 3. get links
#[tokio::main]
async fn main() {
    // read configuration
    let server_config = Config::load().expect("failed to load config");

    // configure log
    let timer = tracing_subscriber::fmt::time::ChronoLocal::rfc_3339();
    let stdout_log = tracing_subscriber::fmt::layer()
        .pretty()
        .with_timer(timer.clone());
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&server_config.log_file)
        .expect("failed to open log file");
    let log_to_file = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_timer(timer)
        .with_writer(log_file);
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "survey_redirect=info".into()),
        ))
        .with(stdout_log)
        .with(log_to_file)
        .init();

    // load state from disk
    let state = RouterState::init(&server_config)
        .await
        .expect("error initing router table");

    // router
    let api = Router::new().route("/", get(handler::redirect));
    let admin = Router::new()
        .route("/get_links", get(handler::get_links))
        .route("/routing_table", put(handler::put_routing_table))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(ValidateRequestHeaderLayer::bearer(
            &server_config.admin_token,
        ));
    let app = Router::new()
        .nest("/api", api)
        .nest("/admin", admin)
        .with_state(state);

    // start service
    tracing::info!("server listening at {}", &server_config.server_binding);
    if let Some(tls) = &server_config.server_tls {
        tracing::info!("serving with secured connections");
        let tls_config = RustlsConfig::from_pem_file(&tls.cert, &tls.key)
            .await
            .expect("invalid tls config");
        axum_server::bind_rustls(server_config.server_binding, tls_config)
            .serve(app.into_make_service())
            .await
            .expect("service failed")
    } else {
        tracing::warn!("serving with insecure connections");
        axum_server::bind(server_config.server_binding)
            .serve(app.into_make_service())
            .await
            .expect("service failed")
    };
}
