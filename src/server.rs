//! All server related code
use crate::config::TlsConfig;
use axum::Router;
use hyper::{body::Incoming, Request};
use hyper_util::rt::{TokioExecutor, TokioIo};
use notify::Watcher;
use rustls_pemfile::{certs, private_key};
use std::{io::BufReader, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tokio_rustls::{
    rustls::{self},
    TlsAcceptor,
};
use tower::Service;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);
const CERT_RETRY_TIMEOUT: Duration = Duration::from_millis(500);

/// run the server loop, handle shudown.
pub async fn run_server(
    app: &Router,
    bind: SocketAddr,
    tls_config: &Option<TlsConfig>,
) -> std::io::Result<()> {
    // attempt to bind to address
    let tcp_listener = TcpListener::bind(bind).await?;
    let shutdown_tx = shutdown_signal();
    let mut tls_acceptor = None;

    // connection counter
    let (close_tx, close_rx) = tokio::sync::watch::channel(());

    // watch cert changes
    let (_watcher, mut cert_update_signal_rx) = watch_cert_changes(tls_config)?;

    // load certs
    update_certs(tls_config, &mut cert_update_signal_rx, &mut tls_acceptor).await;

    tracing::info!("server running");
    loop {
        let new_conn = tokio::select! {
            biased;
            conn = tcp_listener.accept() => conn,
            _ = shutdown_tx.closed() => break,
            _ = cert_update_signal_rx.changed() => {
                tracing::info!("certs change detected, waiting for certs to update");
                tokio::time::sleep(CERT_RETRY_TIMEOUT).await;
                update_certs(tls_config, &mut cert_update_signal_rx, &mut tls_acceptor).await;
                tracing::info!("cert updated");
                continue;
            },
        };

        let (conn, addr) = match new_conn {
            Ok(conn) => conn,
            Err(err) => {
                handle_accept_error(err).await;
                continue;
            }
        };

        let app = app.clone();
        let close_rx = close_rx.clone();
        if let Some(tls) = &tls_acceptor {
            tokio::spawn(handle_conn_tls(app, conn, tls.clone(), close_rx, addr));
        } else {
            tokio::spawn(handle_conn(app, TokioIo::new(conn), close_rx, addr));
        }
    }

    // stop accepting new connections during shutdown periods
    drop(tcp_listener);

    // shutdown procedure: wait for connections to finish
    drop(close_rx);
    tracing::info!(
        "waiting for {} task(s) to finish",
        close_tx.receiver_count()
    );

    // wait for all connections to close
    if timeout(SHUTDOWN_TIMEOUT, close_tx.closed()).await.is_err() {
        tracing::warn!("failed to close all connections");
    }

    Ok(())
}

/// handle tls connection
async fn handle_conn_tls(
    app: Router,
    con: TcpStream,
    tls_acceptor: TlsAcceptor,
    close_rx: tokio::sync::watch::Receiver<()>,
    addr: SocketAddr,
) {
    // tls handshake
    let Ok(stream) = tls_acceptor.accept(con).await else {
        // quickly ignore all tls handshake failure.
        // deny non-secured connections.
        return;
    };
    handle_conn(app, TokioIo::new(stream), close_rx, addr).await;
}

/// serve an incoming connection.
async fn handle_conn<I: hyper::rt::Read + hyper::rt::Write + Unpin + 'static>(
    app: Router,
    stream: I,
    close_rx: tokio::sync::watch::Receiver<()>,
    addr: SocketAddr,
) {
    // Hyper also has its own `Service` trait and doesn't use tower. We can use
    // `hyper::service::service_fn` to create a hyper `Service` that calls our app through
    // `tower::Service::call`.
    let hyper_service = hyper::service::service_fn(move |request: Request<Incoming>| {
        // We have to clone `app` because hyper's `Service` uses `&self` whereas
        // tower's `Service` requires `&mut self`.
        // We don't need to call `poll_ready` since `Router` is always ready.
        let mut app = app.clone();
        app.as_service().call(request)
    });

    if let Err(err) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
        .http1()
        .serve_connection(stream, hyper_service)
        .await
    {
        // skip tls UnexpectedEof:
        // https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof
        if !err
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::UnexpectedEof)
        {
            tracing::trace!("error serving connection from {}: {}", addr, err)
        }
    }

    // decrease connection counter
    drop(close_rx);
}

/// listen to shutdown signals, get `sender.closed()` if signaled.
fn shutdown_signal() -> tokio::sync::watch::Sender<()> {
    let (signal_tx, signal_rx) = tokio::sync::watch::channel(());
    tokio::spawn(async move {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            biased;
            _ = ctrl_c => {},
            _ = terminate => {},
        }

        tracing::info!("received graceful shutdown signal. Telling tasks to shutdown");
        drop(signal_rx);
    });
    signal_tx
}

/// [From `hyper::Server` in 0.14](https://github.com/hyperium/hyper/blob/v0.14.27/src/server/tcp.rs#L186)
///
/// > A possible scenario is that the process has hit the max open files
/// > allowed, and so trying to accept a new connection will fail with
/// > `EMFILE`. In some cases, it's preferable to just wait for some time, if
/// > the application will likely close some files (or connections), and try
/// > to accept the connection again. If this option is `true`, the error
/// > will be logged at the `error` level, since it is still a big deal,
/// > and then the listener will sleep for 1 second.
///
/// hyper allowed customizing this but axum does not.
async fn handle_accept_error(err: std::io::Error) {
    if !is_connection_error(&err) {
        tracing::error!("accept error: {err}");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[inline]
fn is_connection_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::ConnectionRefused
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
    )
}

async fn update_certs(
    tls_config: &Option<TlsConfig>,
    cert_update_signal_rx: &mut tokio::sync::watch::Receiver<()>,
    tls_acceptor: &mut Option<TlsAcceptor>,
) {
    // try to load tls config if any
    let tls_config = load_certs_key(tls_config).await;
    *tls_acceptor = tls_config.map(|tls| TlsAcceptor::from(Arc::new(tls)));

    // clear any extra cert update signal
    cert_update_signal_rx.mark_unchanged();

    // log a warning if notls
    if tls_acceptor.is_none() {
        tracing::warn!("serving with insecured connection.")
    }
}

async fn load_certs_key(config: &Option<TlsConfig>) -> Option<rustls::ServerConfig> {
    loop {
        if let Some(tls_config) = config.as_ref() {
            match tokio::task::block_in_place(|| load_certs_key_sync(tls_config)) {
                Ok(tls_config) => break Some(tls_config),
                Err(e) => {
                    tracing::error!("failed to load certs {}, retrying...", e);
                    tokio::time::sleep(CERT_RETRY_TIMEOUT).await;
                    continue;
                }
            }
        } else {
            break None;
        }
    }
}

/// load certificates and private keys from file (BLOCKING!!).
fn load_certs_key_sync(config: &TlsConfig) -> std::io::Result<rustls::ServerConfig> {
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

/// enable automatic certificate update
fn watch_cert_changes(
    tls_config: &Option<TlsConfig>,
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
    if let Some(config) = tls_config {
        cert_watcher
            .watch(&config.cert, notify::RecursiveMode::NonRecursive)
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("failed to watch cert {}", e),
                )
            })?;
        cert_watcher
            .watch(&config.key, notify::RecursiveMode::NonRecursive)
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("failed to watch key {}", e),
                )
            })?;
    }
    Ok((cert_watcher, cert_update_signal_rx))
}
