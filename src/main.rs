use crate::{certs::cert_provider_from_file, config::Config, state::RouterState};
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, patch, put},
    Router,
};
use std::{fs::OpenOptions, time::Duration};
use tower_http::{
    compression::CompressionLayer, decompression::RequestDecompressionLayer, timeout::TimeoutLayer,
    validate_request::ValidateRequestHeaderLayer,
};
use tracing_subscriber::prelude::*;

pub mod certs;
pub mod config;
pub mod handler;
pub mod server;
pub mod state;
pub mod utility;

pub const EXTERNEL_ID: &str = "externalUserId";
pub const API: &str = "api";
pub const CODE: &str = "code";
pub const CODE_LENGTH: usize = 16;
pub const CONFIG_FILE_NAME: &str = "config.yaml";
pub const BODY_LIMIT: usize = 128 * 1024 * 1024;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

fn main() {
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
    let state = RouterState::init(&server_config).expect("error initing router table");

    // define router
    let app = router(&server_config, state);

    // init runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start runtime");

    let bind = server_config.server_binding;
    tracing::info!("server listening at {}", bind);

    // watch cert changes
    let tls_cert_provider = cert_provider_from_file(
        server_config.server_tls,
        &server_config.watch_cert_changes,
        &rt,
    )
    .expect("failed to watch cert files");

    // start server
    if let Err(e) = rt.block_on(server::run_server(&app, bind, tls_cert_provider)) {
        tracing::error!("failed to run server {}", e);
    }
}

/// define router
fn router(server_config: &Config, state: RouterState) -> Router {
    // define router
    let api = Router::new().route("/", get(handler::redirect));
    let admin = Router::new()
        .route("/get_links", get(handler::get_links))
        .route("/get_codes", get(handler::get_codes))
        .route("/routing_table", put(handler::put_routing_table))
        .route("/routing_table", patch(handler::patch_routing_table))
        .layer(RequestDecompressionLayer::new().gzip(true))
        .layer(CompressionLayer::new().gzip(true))
        .layer(ValidateRequestHeaderLayer::bearer(
            &server_config.admin_token,
        ))
        .layer(DefaultBodyLimit::max(BODY_LIMIT));
    Router::new()
        .nest("/api", api)
        .nest("/admin", admin)
        .layer(TimeoutLayer::new(DEFAULT_TIMEOUT))
        .with_state(state)
}
