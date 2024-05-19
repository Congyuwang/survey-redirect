use crate::config::TlsConfig;
use notify::Watcher as _;
use rustls_pemfile::{certs, private_key};
use std::{io::BufReader, sync::Arc, time::Duration};
use tokio::runtime::Runtime;
use tokio_rustls::{
    rustls::{self},
    TlsAcceptor,
};

const CERT_RETRY_TIMEOUT: Duration = Duration::from_millis(500);

/// Watch the files of the cert, and return a watcher receiver
/// that sends new tls_acceptors when cert file is updated.
/// (involves BLOCKING operations!!!)
pub fn cert_provider_from_file(
    tls_config: Option<TlsConfig>,
    rt: &Runtime,
) -> std::io::Result<Option<tokio::sync::watch::Receiver<TlsAcceptor>>> {
    let Some(tls_config) = tls_config else {
        tracing::warn!("serving with insecured connection.");
        return Ok(None);
    };
    let (watcher, mut cert_update_signal_rx) = watch_cert_changes(&tls_config)?;
    let init_cert = build_tls_acceptor_sync(&tls_config)?;
    let (tls_acceptor_tx, tls_acceptor_rx) = tokio::sync::watch::channel(init_cert);
    rt.spawn(async move {
        // need to keep watcher alive.
        let _watcher = watcher;
        while cert_update_signal_rx.changed().await.is_ok() {
            tracing::info!("certs files change detected");
            // upon cert update signal, wait for some time
            // for cert update tasks to complete
            tokio::time::sleep(CERT_RETRY_TIMEOUT).await;
            let tls_acceptor = build_tls_acceptor(&tls_config).await;
            let _ = tls_acceptor_tx.send(tls_acceptor);
            cert_update_signal_rx.mark_unchanged();
        }
    });
    Ok(Some(tls_acceptor_rx))
}

/// Asynchronous function to load tls files, keep trying if failed.
async fn build_tls_acceptor(tls_config: &TlsConfig) -> TlsAcceptor {
    // try to load tls config if any
    let server_config = loop {
        match tokio::task::block_in_place(|| load_certs_key(tls_config)) {
            Ok(server_config) => break server_config,
            Err(e) => {
                tracing::error!("failed to load certs {}, retrying...", e);
                tokio::time::sleep(CERT_RETRY_TIMEOUT).await;
            }
        }
    };
    TlsAcceptor::from(Arc::new(server_config))
}

/// Synchronous function to load tls files, return error if failed.
/// Not to be used within tokio runtime, but only at the initial stage.
/// (BLOCKING!!)
fn build_tls_acceptor_sync(tls_config: &TlsConfig) -> std::io::Result<TlsAcceptor> {
    // try to load tls config if any
    let tls_config = load_certs_key(tls_config)?;
    Ok(TlsAcceptor::from(Arc::new(tls_config)))
}

/// load certificates and private keys from file (BLOCKING!!).
fn load_certs_key(config: &TlsConfig) -> std::io::Result<rustls::ServerConfig> {
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

    tls_config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(tls_config)
}

/// monitor certificate changes.
fn watch_cert_changes(
    tls_config: &TlsConfig,
) -> std::io::Result<(notify::RecommendedWatcher, tokio::sync::watch::Receiver<()>)> {
    let (cert_update_signal_tx, cert_update_signal_rx) = tokio::sync::watch::channel(());
    let mut cert_watcher =
        notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
            if event.is_ok() {
                let _ = cert_update_signal_tx.send(());
            }
        })
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to init cert watcher {}", e),
            )
        })?;
    cert_watcher
        .watch(&tls_config.cert, notify::RecursiveMode::NonRecursive)
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to watch cert {}", e),
            )
        })?;
    cert_watcher
        .watch(&tls_config.key, notify::RecursiveMode::NonRecursive)
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to watch key {}", e),
            )
        })?;
    Ok((cert_watcher, cert_update_signal_rx))
}
