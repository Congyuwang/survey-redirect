//! All server related code
use crate::config::{Config, TlsConfig};
use axum::Router;
use hyper::{body::Incoming, Request};
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls_pemfile::{certs, private_key};
use std::{io::BufReader, net::SocketAddr, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{
    rustls::{self, ServerConfig},
    TlsAcceptor,
};
use tower::Service;

/// run the server loop, handle shudown.
pub async fn run_server(
    config: &Config,
    app: &Router,
    tls_config: Option<ServerConfig>,
) -> std::io::Result<()> {
    // attempt to bind to address
    let tcp_listener = TcpListener::bind(config.server_binding).await?;
    let shutdown_tx = shutdown_signal();
    let tls_acceptor = tls_config.map(|tls| TlsAcceptor::from(Arc::new(tls)));

    // log a warning if notls
    if tls_acceptor.is_none() {
        tracing::warn!("serving with insecured connection.")
    }

    // connection counter
    let (close_tx, close_rx) = tokio::sync::watch::channel(());

    loop {
        let new_conn = tokio::select! {
            conn = tcp_listener.accept() => conn,
            _ = shutdown_tx.closed() => break,
        };

        let Ok((conn, addr)) = new_conn else {
            continue;
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
    tracing::trace!(
        "waiting for {} task(s) to finish",
        close_tx.receiver_count()
    );
    close_tx.closed().await;

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
    // wait for tls handshake
    let Ok(stream) = tls_acceptor.accept(con).await else {
        tracing::trace!("error during tls handshake connection from {}", addr);
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
            _ = ctrl_c => {},
            _ = terminate => {},
        }

        tracing::trace!("received graceful shutdown signal. Telling tasks to shutdown");
        drop(signal_rx);
    });
    signal_tx
}

/// load certificates and private keys from file (BLOCKING!!).
pub fn load_certs_key(config: &TlsConfig) -> std::io::Result<rustls::ServerConfig> {
    let mut cert = BufReader::new(std::fs::File::open(&config.cert)?);
    let mut key = BufReader::new(std::fs::File::open(&config.key)?);

    let cert_chain = certs(&mut cert).collect::<std::io::Result<Vec<_>>>()?;
    let key_der = private_key(&mut key)?.ok_or(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("private key not found in {}", config.key.display()),
    ))?;

    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key_der)
        .map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("error configuring certs {e}"),
            )
        })?;

    config.alpn_protocols = vec![b"http/1.1".to_vec()];

    Ok(config)
}
