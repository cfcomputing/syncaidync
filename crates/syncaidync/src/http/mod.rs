//! HTTP server: bind loopback, route, shut down cleanly.

pub mod auth;
pub mod error;
pub mod json;
pub mod router;
pub mod sse;
pub mod static_files;
pub mod ws;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;

use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::config::Config;
use crate::registry::Registry;
use crate::reviews::Reviews;
use error::catch_unwind_response;
use sse::Hub;

pub type Body = BoxBody<Bytes, Infallible>;

pub struct AppState {
    pub config: Config,
    pub hub: Hub,
    pub registry: Option<Arc<Registry>>,
    pub reviews: Option<Arc<Reviews>>,
    pub pulls: Option<Arc<crate::pulls::Pulls>>,
    pub history: crate::discovery::History,
    pub links: crate::links::LinkCache,
    pub update: crate::update::UpdateCache,
    bound_port: AtomicU16,
}

impl AppState {
    pub fn new(config: Config, hub: Hub) -> Self {
        let port = config.port;
        Self {
            config,
            hub,
            registry: None,
            reviews: None,
            pulls: None,
            history: Default::default(),
            links: Default::default(),
            update: Default::default(),
            bound_port: AtomicU16::new(port),
        }
    }

    pub fn bound_port(&self) -> u16 {
        self.bound_port.load(Ordering::Relaxed)
    }

    pub fn set_bound_port(&self, port: u16) {
        self.bound_port.store(port, Ordering::Relaxed);
    }

    pub async fn stream_snapshot(&self) -> serde_json::Value {
        let sessions = if let Some(reg) = &self.registry {
            reg.list().await
        } else {
            Vec::new()
        };
        let pulls = if let Some(p) = &self.pulls {
            p.list().await
        } else {
            Vec::new()
        };
        serde_json::json!({
            "sessions": sessions,
            "pulls": pulls,
        })
    }
}

pub fn security_headers(res: &mut Response<Body>, html: bool, port: Option<u16>) {
    use hyper::header::{HeaderName, HeaderValue};
    res.headers_mut().insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    res.headers_mut().insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    if html {
        let p = port.unwrap_or(7717);
        let csp = format!(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' ws://127.0.0.1:{p} ws://localhost:{p}; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"
        );
        if let Ok(v) = HeaderValue::from_str(&csp) {
            res.headers_mut()
                .insert(HeaderName::from_static("content-security-policy"), v);
        }
    }
}

pub fn workspace_media_headers(res: &mut Response<Body>, content_type: &'static str) {
    use hyper::header::{HeaderValue, CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE};
    security_headers(res, false, None);
    res.headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    res.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=60"),
    );
    res.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; sandbox"),
    );
}

pub struct RunningServer {
    pub addr: SocketAddr,
    pub key: String,
    pub state: Arc<AppState>,
    shutdown_tx: watch::Sender<bool>,
    join: tokio::task::JoinHandle<()>,
}

impl RunningServer {
    pub async fn shutdown(self) {
        if let Some(reg) = &self.state.registry {
            reg.shutdown().await;
        }
        let _ = self.shutdown_tx.send(true);
        let _ = self.join.await;
    }
}

/// Bind and serve until shutdown is signalled. Used by the binary and by tests.
pub async fn serve(
    mut config: Config,
) -> Result<RunningServer, Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], config.port))).await?;
    let addr = listener.local_addr()?;
    config.port = addr.port();
    let hub = Hub::new();
    let registry = Registry::load(
        config.home.clone(),
        hub.clone(),
        config.port,
        config.dev_shell,
    )
    .await;
    let reviews = Arc::new(crate::reviews::Reviews::load(&config.home));
    let pulls = Arc::new(crate::pulls::Pulls::load(
        &config.home,
        hub.clone(),
        config.roots.clone(),
    ));
    let mut app = AppState::new(config, hub);
    app.registry = Some(registry);
    app.reviews = Some(reviews);
    app.pulls = Some(pulls);
    app.set_bound_port(addr.port());
    let state = Arc::new(app);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let state_accept = state.clone();
    let mut shutdown_rx_accept = shutdown_rx.clone();

    let join = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_rx_accept.changed() => {
                    if *shutdown_rx_accept.borrow() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else { continue };
                    let state = state_accept.clone();
                    tokio::spawn(async move {
                        let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                            let state = state.clone();
                            async move {
                                catch_unwind_response(router::handle(state, req)).await
                            }
                        });
                        let conn = hyper::server::conn::http1::Builder::new()
                            .serve_connection(TokioIo::new(stream), service)
                            .with_upgrades();
                        let _ = conn.await;
                    });
                }
            }
        }
    });

    // Parent pid watcher
    if let Some(pid) = state.config.parent_pid {
        let tx = shutdown_tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(2));
            loop {
                interval.tick().await;
                // SAFETY: kill with signal 0 only checks whether the process exists.
                let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
                if !alive {
                    let _ = tx.send(true);
                    break;
                }
            }
        });
    }

    Ok(RunningServer {
        addr,
        key: state.config.key.clone(),
        state,
        shutdown_tx,
        join,
    })
}

pub async fn wait_for_ctrl_c(server: RunningServer) {
    let tx = server.shutdown_tx.clone();
    let state = server.state.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = tx.send(true);
    });
    #[cfg(unix)]
    {
        let tx = server.shutdown_tx.clone();
        tokio::spawn(async move {
            if let Ok(mut sig) =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            {
                let _ = sig.recv().await;
                let _ = tx.send(true);
            }
        });
    }
    let _ = server.join.await;
    if let Some(reg) = &state.registry {
        reg.shutdown().await;
    }
}

/// Convenience used when a static HTML response needs the real port in CSP.
pub fn security_headers_with_port(res: &mut Response<Body>, html: bool, port: u16) {
    security_headers(res, html, Some(port));
}
