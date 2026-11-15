//! REST presentation adapter: /api/v1 plus generated OpenAPI documentation.

use std::time::Duration;

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use corvus_application::dto::{LoginRequest, RefreshRequest};
use corvus_application::ToolCall;
use corvus_domain::recording::{SessionRow, StepRow};
use corvus_proto::{ErrorBody, ErrorCode};
use serde::Deserialize;
use serde_json::{json, Value};
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::auth::{AuthError, AuthService};
use crate::state::AppState;

/// API failure with a stable JSON error envelope.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    body: Value,
}

impl ApiError {
    /// Build a BAD_ARGS failure.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::from_error_body(ErrorBody::new(ErrorCode::BadArgs, message))
    }

    /// Build a NOT_FOUND failure.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::from_error_body(ErrorBody::new(ErrorCode::NotFound, message))
    }

    /// Wrap a typed tool error.
    pub fn from_error_body(error: ErrorBody) -> Self {
        let status = status_for(error.code);
        Self {
            status,
            body: json!({ "error": error }),
        }
    }

    /// Wrap a repository failure as an internal error.
    pub fn storage(message: String) -> Self {
        Self::from_error_body(ErrorBody::new(ErrorCode::Internal, message))
    }

    /// Wrap an auth failure.
    pub fn from_auth(error: AuthError) -> Self {
        let body = json!({
            "error": {
                "code": error.code(),
                "message": error.to_string(),
            }
        });
        Self {
            status: error.status(),
            body,
        }
    }

    /// HTTP status.
    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

fn status_for(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::BadArgs => StatusCode::BAD_REQUEST,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::NotInteractable => StatusCode::CONFLICT,
        ErrorCode::Timeout => StatusCode::GATEWAY_TIMEOUT,
        ErrorCode::NavigationLost => StatusCode::CONFLICT,
        ErrorCode::CapabilityUnavailable => StatusCode::NOT_IMPLEMENTED,
        ErrorCode::BrowserGone | ErrorCode::TransportDown => StatusCode::BAD_GATEWAY,
        ErrorCode::Internal | ErrorCode::Cancelled => StatusCode::INTERNAL_SERVER_ERROR,
        ErrorCode::PolicyDenied => StatusCode::FORBIDDEN,
    }
}

/// Pagination and time floor for the session list.
#[derive(Debug, Deserialize)]
pub struct SessionsQuery {
    /// Page size, clamped to 1..=500.
    pub limit: Option<u32>,
    /// Only sessions starting at or after this epoch millisecond.
    pub since_ms: Option<i64>,
}

/// Pagination for one session's steps.
#[derive(Debug, Deserialize)]
pub struct StepsQuery {
    /// First sequence number to return.
    pub from_seq: Option<i64>,
    /// Page size, clamped to 1..=500.
    pub limit: Option<u32>,
}

/// Tool catalogue detail selector.
#[derive(Debug, Deserialize)]
pub struct ToolsQuery {
    /// brief or full; brief is the default.
    pub detail: Option<String>,
}

/// One REST tool call.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ToolCallBody {
    /// Tool name from the catalogue.
    pub name: String,
    /// Tool arguments, validated by the shared validator.
    #[serde(default)]
    pub arguments: Value,
    /// Optional browser id merged into the arguments.
    #[serde(default)]
    pub browser_id: Option<String>,
    /// Optional MCP session id.
    #[serde(default)]
    pub session_id: Option<String>,
}

fn clamped(limit: Option<u32>, default: u32, max: u32) -> u32 {
    limit.unwrap_or(default).clamp(1, max)
}

fn session_json(row: &SessionRow) -> Value {
    json!({
        "id": row.id,
        "started_at": row.started_at,
        "ended_at": row.ended_at,
        "browser": row.browser,
        "profile": row.profile,
        "agent": row.agent,
        "transport": row.transport,
        "status": row.status,
        "steps": row.steps,
        "errors": row.errors,
    })
}

fn json_field(raw: &Option<String>) -> Value {
    raw.as_deref()
        .map(|text| {
            serde_json::from_str::<Value>(text).unwrap_or_else(|_| Value::String(text.to_string()))
        })
        .unwrap_or(Value::Null)
}

fn step_json(row: &StepRow) -> Value {
    json!({
        "id": row.id,
        "session_id": row.session_id,
        "seq": row.seq,
        "ts": row.ts,
        "tool": row.tool,
        "args": json_field(&row.args_json),
        "result": json_field(&row.result_json),
        "error_code": row.error_code,
        "duration_ms": row.duration_ms,
        "browser_id": row.browser_id,
        "tab_id": row.tab_id,
        "ok": row.ok,
        "args_truncated": row.args_truncated,
        "result_truncated": row.result_truncated,
    })
}

/// Health projection with the API build version.
#[utoipa::path(
    get,
    path = "/api/v1/health",
    responses((status = 200, description = "Health projection"))
)]
pub async fn health(State(state): State<AppState>) -> Json<Value> {
    let health = (state.health)();
    Json(json!({ "ok": true, "version": state.version, "health": health }))
}

/// Registered browsers.
#[utoipa::path(
    get,
    path = "/api/v1/browsers",
    responses((status = 200, description = "Registered browsers"))
)]
pub async fn list_browsers(State(state): State<AppState>) -> Json<Value> {
    let browsers = state.browsers.list().await;
    Json(json!({ "browsers": browsers, "count": browsers.len() }))
}

/// One registered browser.
#[utoipa::path(
    get,
    path = "/api/v1/browsers/{id}",
    params(("id" = String, Path, description = "Browser id")),
    responses((status = 200, description = "Browser"), (status = 404, description = "Unknown browser"))
)]
pub async fn get_browser(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    match state.browsers.get(&id).await {
        Some(browser) => Ok(Json(json!({ "browser": browser }))),
        None => Err(ApiError::not_found(format!(
            "browser {id} is not connected"
        ))),
    }
}

/// Recent recording sessions.
#[utoipa::path(
    get,
    path = "/api/v1/sessions",
    params(("limit" = Option<u32>, Query, description = "Page size"), ("since_ms" = Option<i64>, Query, description = "Start time floor")),
    responses((status = 200, description = "Sessions"))
)]
pub async fn list_sessions(
    State(state): State<AppState>,
    Query(query): Query<SessionsQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = clamped(query.limit, 50, 500);
    let rows = state
        .recordings
        .list_sessions(limit, query.since_ms)
        .await
        .map_err(ApiError::storage)?;
    let sessions: Vec<Value> = rows.iter().map(session_json).collect();
    Ok(Json(
        json!({ "sessions": sessions, "count": sessions.len() }),
    ))
}

/// One recording session.
#[utoipa::path(
    get,
    path = "/api/v1/sessions/{id}",
    params(("id" = String, Path, description = "Session id")),
    responses((status = 200, description = "Session"), (status = 404, description = "Unknown session"))
)]
pub async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    match state
        .recordings
        .get_session(&id)
        .await
        .map_err(ApiError::storage)?
    {
        Some(row) => Ok(Json(json!({ "session": session_json(&row) }))),
        None => Err(ApiError::not_found(format!("session {id} does not exist"))),
    }
}

/// Ordered steps for one session.
#[utoipa::path(
    get,
    path = "/api/v1/sessions/{id}/steps",
    params(("id" = String, Path, description = "Session id"), ("from_seq" = Option<i64>, Query, description = "First sequence"), ("limit" = Option<u32>, Query, description = "Page size")),
    responses((status = 200, description = "Steps"))
)]
pub async fn get_session_steps(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<StepsQuery>,
) -> Result<Json<Value>, ApiError> {
    let limit = clamped(query.limit, 100, 500);
    let rows = state
        .recordings
        .get_steps(&id, query.from_seq.unwrap_or(0), limit)
        .await
        .map_err(ApiError::storage)?;
    let steps: Vec<Value> = rows.iter().map(step_json).collect();
    Ok(Json(
        json!({ "session_id": id, "steps": steps, "count": steps.len() }),
    ))
}

/// The MCP tool catalogue in brief or full detail.
#[utoipa::path(
    get,
    path = "/api/v1/tools",
    params(("detail" = Option<String>, Query, description = "brief or full")),
    responses((status = 200, description = "Tool catalogue"))
)]
pub async fn list_tools(
    State(state): State<AppState>,
    Query(query): Query<ToolsQuery>,
) -> Json<Value> {
    let detail = query.detail.as_deref().unwrap_or("brief");
    let tools = state.catalog.definitions_for(None, detail);
    Json(json!({ "tools": tools, "count": tools.len() }))
}

/// Execute one validated tool call through the shared application service.
#[utoipa::path(
    post,
    path = "/api/v1/tools/call",
    responses((status = 200, description = "Tool result"), (status = 400, description = "Invalid arguments"))
)]
pub async fn call_tool(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ToolCallBody>,
) -> Result<Json<Value>, ApiError> {
    state.authorize(&headers).map_err(ApiError::from_auth)?;
    let mut arguments = match body.arguments {
        Value::Null => json!({}),
        Value::Object(map) => Value::Object(map),
        _ => return Err(ApiError::bad_request("arguments must be a JSON object")),
    };
    if let Some(browser_id) = body.browser_id {
        if let Value::Object(map) = &mut arguments {
            map.insert("browser_id".to_string(), Value::String(browser_id));
        }
    }
    let name = body.name;
    let arguments = state
        .catalog
        .validate(&name, &arguments)
        .map_err(ApiError::from_error_body)?;
    let session = None;
    let _ = body.session_id;
    let result = state
        .tools
        .call(ToolCall {
            name,
            arguments,
            session,
        })
        .await;
    match result {
        Ok(data) => Ok(Json(json!({ "ok": true, "data": data }))),
        Err(error) => Err(ApiError::from_error_body(error)),
    }
}

/// Exchange credentials for a token pair.
#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    responses((status = 200, description = "Token pair"), (status = 401, description = "Invalid credentials"))
)]
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<corvus_application::dto::TokenPair>, ApiError> {
    let request: LoginRequest = serde_json::from_value(body)
        .map_err(|error| ApiError::bad_request(format!("invalid login body: {error}")))?;
    let auth = require_auth(&state)?;
    let pair = auth.login(&request).await.map_err(ApiError::from_auth)?;
    Ok(Json(pair))
}

/// Rotate a refresh token.
#[utoipa::path(
    post,
    path = "/api/v1/auth/refresh",
    responses((status = 200, description = "New token pair"), (status = 401, description = "Invalid refresh token"))
)]
pub async fn refresh(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<corvus_application::dto::TokenPair>, ApiError> {
    let request: RefreshRequest = serde_json::from_value(body)
        .map_err(|error| ApiError::bad_request(format!("invalid refresh body: {error}")))?;
    let auth = require_auth(&state)?;
    let pair = auth.refresh(&request).await.map_err(ApiError::from_auth)?;
    Ok(Json(pair))
}

/// Revoke a refresh token.
#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    responses((status = 200, description = "Logged out"))
)]
pub async fn logout(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let request: RefreshRequest = serde_json::from_value(body)
        .map_err(|error| ApiError::bad_request(format!("invalid logout body: {error}")))?;
    let auth = require_auth(&state)?;
    auth.logout(&request).await.map_err(ApiError::from_auth)?;
    Ok(Json(json!({ "ok": true })))
}

/// Claims of the current bearer token.
#[utoipa::path(
    get,
    path = "/api/v1/auth/me",
    responses((status = 200, description = "Current user"), (status = 401, description = "Missing token"))
)]
pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let claims = state.authorize(&headers).map_err(ApiError::from_auth)?;
    match claims {
        Some(claims) => Ok(Json(json!({
            "user": { "id": claims.subject, "username": claims.username, "role": claims.role },
            "expires_at": claims.expires_at,
        }))),
        None => Err(ApiError::from_auth(AuthError::Disabled)),
    }
}

fn require_auth(state: &AppState) -> Result<&Arc<AuthService>, ApiError> {
    state
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::from_auth(AuthError::Disabled))
}

use std::sync::Arc;

/// Generated OpenAPI document.
#[derive(OpenApi)]
#[openapi(
    paths(
        health, list_browsers, get_browser, list_sessions, get_session,
        get_session_steps, list_tools, call_tool, login, refresh, logout, me
    ),
    tags((name = "corvus", description = "Corvus browser automation API"))
)]
pub struct ApiDoc;

async fn auth_guard(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if path == "/api/v1/auth/login" || path == "/api/v1/auth/refresh" || path == "/api/v1/health" {
        return next.run(request).await;
    }
    match state.authorize(request.headers()) {
        Ok(_) => next.run(request).await,
        Err(error) => ApiError::from_auth(error).into_response(),
    }
}

fn cors_layer() -> CorsLayer {
    let origins = AllowOrigin::list([
        HeaderValue::from_static("http://localhost"),
        HeaderValue::from_static("http://127.0.0.1"),
        HeaderValue::from_static("http://[::1]"),
    ]);
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any)
}

const REQUEST_ID: &str = "x-request-id";

/// Build the /api/v1 router with Swagger UI and HTTP middleware.
pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/browsers", get(list_browsers))
        .route("/api/v1/browsers/{id}", get(get_browser))
        .route("/api/v1/sessions", get(list_sessions))
        .route("/api/v1/sessions/{id}", get(get_session))
        .route("/api/v1/sessions/{id}/steps", get(get_session_steps))
        .route("/api/v1/tools", get(list_tools))
        .route("/api/v1/tools/call", post(call_tool))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/refresh", post(refresh))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/auth/me", get(me))
        .layer(middleware::from_fn_with_state(state.clone(), auth_guard))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(120),
        ))
        .layer(CompressionLayer::new())
        .layer(PropagateRequestIdLayer::new(HeaderName::from_static(
            REQUEST_ID,
        )))
        .layer(SetRequestIdLayer::new(
            HeaderName::from_static(REQUEST_ID),
            MakeRequestUuid,
        ))
        .layer(cors_layer())
        .with_state(state);

    api.merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use corvus_application::dto::BrowserSummary;
    use corvus_application::ports::{BrowserDirectory, RecordingRepository, ToolCatalog};
    use corvus_application::{BackendFuture, ToolBackend, ToolCall};
    use corvus_domain::recording::{SessionRow, StepRecord, StepRow};
    use corvus_proto::ErrorBody;
    use tower::ServiceExt;

    use crate::auth::{AuthConfig, MemoryAuthRepository};

    struct EmptyBrowsers;

    #[async_trait]
    impl BrowserDirectory for EmptyBrowsers {
        async fn list(&self) -> Vec<BrowserSummary> {
            Vec::new()
        }

        async fn get(&self, _id: &str) -> Option<BrowserSummary> {
            None
        }
    }

    struct EmptyRecordings;

    #[async_trait]
    impl RecordingRepository for EmptyRecordings {
        async fn list_sessions(
            &self,
            _limit: u32,
            _since_ms: Option<i64>,
        ) -> Result<Vec<SessionRow>, String> {
            Ok(Vec::new())
        }

        async fn get_session(&self, _id: &str) -> Result<Option<SessionRow>, String> {
            Ok(None)
        }

        async fn get_steps(
            &self,
            _session_id: &str,
            _from_seq: i64,
            _limit: u32,
        ) -> Result<Vec<StepRow>, String> {
            Ok(Vec::new())
        }

        async fn record_step(&self, _step: StepRecord) -> Result<(), String> {
            Ok(())
        }
    }

    struct EchoCatalog;

    impl ToolCatalog for EchoCatalog {
        fn definitions_for(&self, _category: Option<&str>, _detail: &str) -> Vec<Value> {
            Vec::new()
        }

        fn validate(&self, _name: &str, arguments: &Value) -> Result<Value, ErrorBody> {
            Ok(arguments.clone())
        }
    }

    struct EchoTools;

    impl ToolBackend for EchoTools {
        fn call(&self, _call: ToolCall) -> BackendFuture {
            Box::pin(async { Ok(json!({ "clicked": true })) })
        }
    }

    fn state(auth: Option<Arc<AuthService>>) -> AppState {
        let health = Arc::new(|| json!({ "status": "ok" }));
        AppState::new(
            Arc::new(EmptyBrowsers),
            Arc::new(EmptyRecordings),
            Arc::new(EchoTools),
            Arc::new(EchoCatalog),
            health,
            auth,
            "1.4.0-test",
        )
    }

    async fn body_json(response: axum::response::Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[tokio::test]
    async fn health_reports_the_build_and_needs_no_auth() {
        let response = router(state(None))
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let value = body_json(response).await;
        assert_eq!(value["version"], "1.4.0-test");
        assert_eq!(value["health"]["status"], "ok");
    }

    #[tokio::test]
    async fn auth_guard_challenges_and_login_issues_a_usable_token() {
        let accounts = Arc::new(MemoryAuthRepository::new());
        accounts
            .add_user("admin", "secret", "admin")
            .await
            .expect("user");
        let auth = Arc::new(AuthService::new(
            AuthConfig::local("0123456789abcdef0123456789abcdef"),
            accounts,
        ));
        let app = router(state(Some(auth)));
        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/browsers")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        let login = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/auth/login")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"username":"admin","password":"secret"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(login.status(), StatusCode::OK);
        let pair = body_json(login).await;
        let token = pair["access_token"].as_str().expect("access token");
        let authorized = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/browsers")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(authorized.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn tool_calls_validate_and_use_the_shared_executor() {
        let response = router(state(None))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/tools/call")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"name":"browser_click","arguments":{"target":"@c1"}}"#,
                    ))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let value = body_json(response).await;
        assert_eq!(value["ok"], true);
        assert_eq!(value["data"]["clicked"], true);
    }
}
