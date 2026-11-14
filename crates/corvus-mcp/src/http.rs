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
            .route("/", get(banner))
            .with_state(state);
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
    let session = state.server.session_id_for(&body);
    match state.server.handle_message(&body).await {
        Some(text) => {
            let mut response = (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/json")],
                text,
            )
                .into_response();
            if let Some(session) = session {
                if let Ok(value) = HeaderValue::from_str(&session) {
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

async fn mcp_delete() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(header::ALLOW, "POST")],
        "sessions are not persistent; there is nothing to delete",
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

async fn banner() -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        "corvus mcp server\n\nPOST JSON-RPC 2.0 messages to /mcp\nGET /health for daemon status\n",
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
