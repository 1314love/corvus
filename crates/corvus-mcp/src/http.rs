//! Streamable HTTP transport.
//!
//! One stateless POST endpoint at /mcp, a health document for the doctor
//! command and a plain text banner at the root. Server initiated messages are
//! not used, so a GET on /mcp answers 405 as the specification requires.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::protocol::McpServer;

struct HttpState {
    server: Arc<McpServer>,
}

/// A bound HTTP listener waiting to be served.
pub struct McpHttpServer {
    listener: TcpListener,
    local_addr: SocketAddr,
    router: Router,
}

impl McpHttpServer {
    /// Bind the listener. Use port 0 to let the operating system choose.
    pub async fn bind(addr: SocketAddr, server: Arc<McpServer>) -> std::io::Result<Self> {
        Self::bind_with(addr, server, Router::new()).await
    }

    /// Bind the listener and merge an extra router, so REST and MCP share one port.
    pub async fn bind_with(
        addr: SocketAddr,
        server: Arc<McpServer>,
        extra: Router,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let state = Arc::new(HttpState { server });
        let router = Router::new()
            .route(
                "/mcp",
                axum::routing::post(mcp_post)
                    .get(mcp_get)
                    .delete(mcp_delete),
            )
            .route("/health", get(health))
            .route("/metrics", get(metrics))
            .route("/", get(banner))
            .with_state(state)
            .merge(extra);
        Ok(Self {
            listener,
            local_addr,
            router,
        })
    }

    /// The address the listener is bound to.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Serve until the shutdown signal flips.
    pub async fn serve(self, mut shutdown: watch::Receiver<bool>) {
        let addr = self.local_addr;
        tracing::info!(url = %format!("http://{addr}/mcp"), "mcp http listening");
        let signal = async move {
            if !*shutdown.borrow() {
                let _ = shutdown.changed().await;
            }
        };
        if let Err(error) = axum::serve(self.listener, self.router)
            .with_graceful_shutdown(signal)
            .await
        {
            tracing::warn!(error = %error, "mcp http server stopped");
        }
    }

    /// Serve on a background task.
    pub fn spawn(self, shutdown: watch::Receiver<bool>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(self.serve(shutdown))
    }
}

async fn mcp_post(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    if !origin_allowed(&headers) {
        return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
    }
    let opened = state.server.initialize_session(&body, "http");
    let existing = headers
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|session_id| state.server.session_context(session_id, "http"));
    let session = opened.clone().or(existing);
    match state
        .server
        .handle_message_with(&body, session.as_ref())
        .await
    {
        Some(text) => {
            let mut response = (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                text,
            )
                .into_response();
            if let Some(session) = opened {
                if let Ok(value) = HeaderValue::from_str(&session.session_id) {
                    response.headers_mut().insert("mcp-session-id", value);
                }
            }
            response
        }
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn mcp_get() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "POST, DELETE")],
        "this server does not offer a server initiated event stream; POST to /mcp",
    )
        .into_response()
}

async fn mcp_delete(State(state): State<Arc<HttpState>>, headers: HeaderMap) -> Response {
    let Some(session_id) = headers
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
    else {
        return (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            "{\"error\":\"mcp-session-id header required\"}",
        )
            .into_response();
    };
    state.server.cancel_session(&session_id);
    state.server.finish_session(&session_id, "closed");
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        format!("{{\"closed\":true,\"session_id\":{session_id:?}}}"),
    )
        .into_response()
}

async fn health(State(state): State<Arc<HttpState>>) -> Response {
    let body = serde_json::to_string(&state.server.health()).unwrap_or_else(|_| "{}".to_string());
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

async fn metrics(State(state): State<Arc<HttpState>>) -> Response {
    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        state.server.metrics_text(),
    )
        .into_response()
}

async fn banner() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "corvus mcp server\n\nPOST JSON-RPC 2.0 messages to /mcp\nGET /health for daemon status\nGET /metrics for Prometheus counters\n",
    )
        .into_response()
}

fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    origin.starts_with("http://localhost")
        || origin.starts_with("http://127.0.0.1")
        || origin.starts_with("http://[::1]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_local_origins_pass_the_check() {
        let mut headers = HeaderMap::new();
        assert!(origin_allowed(&headers));
        for origin in [
            "http://localhost:7331",
            "http://127.0.0.1:3000",
            "http://[::1]:9",
        ] {
            headers.insert(header::ORIGIN, HeaderValue::from_static(origin));
            assert!(origin_allowed(&headers), "{origin} should be allowed");
        }
        for origin in ["http://evil.example", "https://localhost:7331"] {
            headers.insert(header::ORIGIN, HeaderValue::from_static(origin));
            assert!(!origin_allowed(&headers), "{origin} should be refused");
        }
    }
}
