//! All server related code
use crate::config::{Config, TlsConfig};
use axum::Router;
use hyper::{body::Incoming, Request};
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls_pemfile::{certs, private_key};
use std::{io::BufReader, net::SocketAddr, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{
    rustls::{self},
    TlsAcceptor,
};
use tower::Service;

pub async fn start_server_nontls(config: &Config, app: &Router) -> std::io::Result<()> {
    // attempt to bind to address
    let tcp_listener = TcpListener::bind(config.server_binding).await?;

    loop {
        // accept new connection
        let Ok((con, addr)) = tcp_listener.accept().await else {
            tracing::error!("error accepting connection");
            continue;
        };
        tracing::debug!("new connection from {}", addr);

        let app = app.clone();
        tokio::spawn(async move {
            handle_conn_nontls(app, con, &addr).await;
        });
    }
}

pub async fn start_server_tls(
    config: &Config,
    app: &Router,
    tls_config: rustls::ServerConfig,
) -> std::io::Result<()> {
    // attempt to bind to address
    let tcp_listener = TcpListener::bind(config.server_binding).await?;
    let tls_acceptor = TlsAcceptor::from(Arc::new(tls_config));

    loop {
        // accept new connection
        let Ok((con, addr)) = tcp_listener.accept().await else {
            tracing::error!("error accepting connection");
            continue;
        };
        tracing::debug!("new connection from {}", addr);

        let app = app.clone();
        let tls_acceptor = tls_acceptor.clone();
        tokio::spawn(async move {
            handle_conn_tls(app, con, tls_acceptor, &addr).await;
        });
    }
}

/// handle non-tls connection
async fn handle_conn_nontls(app: Router, con: TcpStream, addr: &SocketAddr) {
    // Hyper has its own `AsyncRead` and `AsyncWrite` traits and doesn't use tokio.
    serve_conn(app, TokioIo::new(con), addr).await;
}

/// handle tls connection
async fn handle_conn_tls(
    app: Router,
    con: TcpStream,
    tls_acceptor: TlsAcceptor,
    addr: &SocketAddr,
) {
    // wait for tls handshake
    let Ok(stream) = tls_acceptor.accept(con).await else {
        tracing::error!("error during tls handshake connection from {}", addr);
        return;
    };
    serve_conn(app, TokioIo::new(stream), addr).await;
}

/// serve an incoming connection.
async fn serve_conn<I: hyper::rt::Read + hyper::rt::Write + Unpin + 'static>(
    app: Router,
    stream: I,
    addr: &SocketAddr,
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

    if let Err(e) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
        .http1()
        .serve_connection(stream, hyper_service)
        .await
    {
        tracing::error!("error serving connection from {}: {}", addr, e);
    }
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
