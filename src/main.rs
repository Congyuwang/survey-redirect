use crate::{config::Config, state::RouterState};
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, patch, put},
    Router,
};
use config::TlsConfig;
use notify::Watcher;
use rustls_pemfile::{certs, private_key};
use std::{fs::OpenOptions, io::BufReader, time::Duration};
use tokio_rustls::rustls;
use tower_http::{
    compression::CompressionLayer, decompression::RequestDecompressionLayer, timeout::TimeoutLayer,
    validate_request::ValidateRequestHeaderLayer,
};
use tracing_subscriber::prelude::*;

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
    let (restart_signal_tx, mut restart_signal_rx) = tokio::sync::watch::channel(());
    let mut cert_watcher =
        notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
            if event.is_ok() {
                let _ = restart_signal_tx.send(());
            }
        })
        .expect("failed to start watcher");

    const CERTS_RETRY_TIMEOUT: Duration = Duration::from_secs(5);

    loop {
        // try to load tls config if any
        let tls_config = match server_config
            .server_tls
            .as_ref()
            .map(|tls| load_certs_key(tls, &mut cert_watcher))
        {
            Some(Ok(tls_config)) => Some(tls_config),
            Some(Err(e)) => {
                tracing::error!("failed to load certs {}, retrying...", e);
                std::thread::sleep(CERTS_RETRY_TIMEOUT);
                continue;
            }
            None => None,
        };

        tracing::info!("server running");
        let should_restart = rt
            .block_on(server::run_server(
                &app,
                bind,
                tls_config,
                restart_signal_rx.clone(),
            ))
            .expect("failed to bind to address");

        if should_restart {
            tracing::info!("certs change detected, waiting for certs to update");
            std::thread::sleep(CERTS_RETRY_TIMEOUT);
            // clear restart signal queue, before restarting
            restart_signal_rx.mark_unchanged();
        } else {
            break;
        }
    }
}

/// define router
fn router(server_config: &Config, state: RouterState) -> Router {
    // define router
    let api = Router::new().route("/", get(handler::redirect));
    let admin = Router::new()
        .route("/get_links", get(handler::get_links))
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

/// load certificates and private keys from file (BLOCKING!!).
pub fn load_certs_key(
    config: &TlsConfig,
    watcher: &mut notify::RecommendedWatcher,
) -> std::io::Result<rustls::ServerConfig> {
    let mut cert = BufReader::new(std::fs::File::open(&config.cert)?);
    let mut key = BufReader::new(std::fs::File::open(&config.key)?);

    let cert_chain = certs(&mut cert).collect::<std::io::Result<Vec<_>>>()?;
    let key_der = private_key(&mut key)?.ok_or(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("private key not found in {}", config.key.display()),
    ))?;

    let mut tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key_der)
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("error configuring certs {e}"),
            )
        })?;

    watcher
        .watch(&config.cert, notify::RecursiveMode::NonRecursive)
        .expect("failed to watch cert");
    watcher
        .watch(&config.key, notify::RecursiveMode::NonRecursive)
        .expect("failed to watch cert");

    tls_config.alpn_protocols = vec![b"http/1.1".to_vec()];

    Ok(tls_config)
}
