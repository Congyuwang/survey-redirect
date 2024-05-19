//! All server related code
use crate::DEFAULT_TIMEOUT;
use axum::Router;
use hyper::{body::Incoming, Request};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tokio_rustls::TlsAcceptor;
use tower::Service;

/// run the server loop, handle shudown.
pub async fn run_server(
    app: &Router,
    bind: SocketAddr,
    mut tls_cert_provider: Option<tokio::sync::watch::Receiver<TlsAcceptor>>,
) -> std::io::Result<()> {
    // attempt to bind to address
    let tcp_listener = TcpListener::bind(bind).await?;
    // shutdown signal
    let shutdown_tx = shutdown_signal();
    // connection counter
    let (close_tx, close_rx) = tokio::sync::watch::channel(());

    // main loop
    tracing::info!("server running");
    if let Some(tls_cert_provider) = tls_cert_provider.as_mut() {
        server_loop(
            &tcp_listener,
            &shutdown_tx,
            &close_rx,
            tls_cert_provider,
            app,
        )
        .await
    } else {
        server_loop_notls(&tcp_listener, &shutdown_tx, &close_rx, app).await
    }

    // graceful shutdown process

    // stop accepting new connections during shutdown periods
    drop(tcp_listener);
    // shutdown procedure: wait for connections to finish
    drop(close_rx);
    // wait for all connections to close
    tracing::info!(
        "waiting for {} task(s) to finish",
        close_tx.receiver_count()
    );
    if timeout(DEFAULT_TIMEOUT, close_tx.closed()).await.is_err() {
        tracing::warn!("failed to close all connections");
    }
    Ok(())
}

/// run the server loop, no tls, handle shudown.
pub async fn server_loop(
    tcp_listener: &TcpListener,
    shutdown_tx: &tokio::sync::watch::Sender<()>,
    close_rx: &tokio::sync::watch::Receiver<()>,
    tls_cert_provider: &mut tokio::sync::watch::Receiver<TlsAcceptor>,
    app: &Router,
) {
    let mut tls_acceptor = tls_cert_provider.borrow_and_update().clone();
    loop {
        let new_conn = tokio::select! {
            biased;
            conn = tcp_listener.accept() => conn,
            Ok(_) = tls_cert_provider.changed() => {
                tls_acceptor = tls_cert_provider.borrow_and_update().clone();
                tracing::info!("cert updated");
                continue;
            }
            _ = shutdown_tx.closed() => break,
            else => continue,
        };

        let (conn, addr) = match new_conn {
            Ok(conn) => conn,
            Err(err) => {
                handle_accept_error(err).await;
                continue;
            }
        };

        let app = app.clone();
        let tls_acceptor = tls_acceptor.clone();
        let close_rx = close_rx.clone();
        tokio::spawn(handle_conn_tls(app, conn, tls_acceptor, close_rx, addr));
    }
}

/// run the server loop, no tls, handle shudown.
pub async fn server_loop_notls(
    tcp_listener: &TcpListener,
    shutdown_tx: &tokio::sync::watch::Sender<()>,
    close_rx: &tokio::sync::watch::Receiver<()>,
    app: &Router,
) {
    loop {
        let new_conn = tokio::select! {
            biased;
            conn = tcp_listener.accept() => conn,
            _ = shutdown_tx.closed() => break,
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
        tokio::spawn(handle_conn(app, TokioIo::new(conn), close_rx, addr));
    }
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
