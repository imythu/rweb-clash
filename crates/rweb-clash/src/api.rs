use crate::app::App;
use crate::error::{scope_trace_id, AppError};
use crate::types::{
    FilterRuleInput, ManualNodeInput, OperationResponse, ProxyGroupRequest, RuleInput,
    RuleSetInput, RuleTestRequest, SelectProxyRequest, SubscriptionInput, SystemConfigPatch,
    WebDavSettingsInput,
};
use axum::body::{to_bytes, Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::time::Instant;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tracing::{error, info, info_span, warn, Instrument};
use uuid::Uuid;

static TRACE_ID_HEADER: HeaderName = HeaderName::from_static("x-trace-id");
const MAX_API_BODY_BYTES: usize = 1024 * 1024;
const DEFAULT_ALLOWED_ORIGINS: &[&str] = &[
    "tauri://localhost",
    "http://tauri.localhost",
    "http://localhost:5173",
    "http://127.0.0.1:5173",
];

pub fn router(app: App) -> Router {
    let api = Router::new()
        .route("/api/configs", get(get_configs).patch(patch_configs))
        .route("/api/system/status", get(system_status))
        .route("/api/setup/status", get(setup_status))
        .route("/api/system/egress", get(system_egress))
        .route("/api/core/status", get(core_status))
        .route("/api/core/start", post(core_start))
        .route("/api/core/stop", post(core_stop))
        .route("/api/core/restart", post(core_restart))
        .route(
            "/api/subscriptions",
            get(list_subscriptions).post(create_subscription),
        )
        .route(
            "/api/subscriptions/{id}",
            patch(update_subscription).delete(delete_subscription),
        )
        .route("/api/subscriptions/{id}/members", get(subscription_members))
        .route(
            "/api/subscriptions/{id}/refresh",
            post(refresh_subscription),
        )
        .route(
            "/api/subscription-rules/global",
            get(global_filter_rules).put(replace_global_filter_rules),
        )
        .route("/api/proxies", get(proxy_topology).post(create_proxy_group))
        .route(
            "/api/proxies/{group}",
            put(update_or_select_proxy).delete(delete_proxy_group),
        )
        .route("/api/proxies/{group}/test", post(test_proxy_group))
        .route("/api/nodes/test", post(test_node))
        .route(
            "/api/manual-nodes",
            get(list_manual_nodes).post(create_manual_node),
        )
        .route(
            "/api/manual-nodes/{name}",
            put(update_manual_node).delete(delete_manual_node),
        )
        .route("/api/rules", get(list_rules).post(create_rule))
        .route("/api/rules/{id}", put(update_rule).delete(delete_rule))
        .route("/api/rules/test", post(test_rule))
        .route("/api/rule-sets", get(list_rule_sets).post(create_rule_set))
        .route("/api/rule-sets/{id}/refresh", post(refresh_rule_set))
        .route("/api/rule-sets/{id}", delete(delete_rule_set))
        .route("/api/logs", get(list_logs).delete(clear_logs))
        .route("/api/logs/export", get(export_logs))
        .route("/api/diagnostics/export", get(export_diagnostics))
        .route("/api/backups", get(list_backups).post(create_backup))
        .route("/api/backups/{name}", delete(delete_backup))
        .route("/api/backups/{name}/restore", post(restore_backup))
        .route(
            "/api/webdav",
            get(webdav_settings).put(save_webdav_settings),
        )
        .route("/api/webdav/test", post(test_webdav))
        .route("/api/webdav/sync", post(sync_webdav))
        .route("/api/webdav/restore", post(restore_webdav))
        .route("/api/traffic", get(traffic))
        .route(
            "/api/connections",
            get(connections).delete(close_all_connections),
        )
        .route("/api/connections/{id}", delete(close_connection))
        .route("/api/dns/flush", post(flush_dns))
        .layer(DefaultBodyLimit::max(MAX_API_BODY_BYTES))
        .layer(middleware::from_fn(require_api_auth))
        .layer(middleware::from_fn(trace_request));

    let router = if app
        .embedded_assets()
        .is_some_and(|assets| assets.has_prefix("web/"))
    {
        api.fallback(get(embedded_frontend))
    } else {
        let frontend_dist = app.paths().frontend_dist.clone();
        let static_files = ServeDir::new(frontend_dist.clone())
            .not_found_service(ServeFile::new(frontend_dist.join("index.html")));
        api.fallback_service(static_files)
    };

    router.with_state(app).layer(cors_layer())
}

fn cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(allowed_origins())
        .allow_headers([
            header::ACCEPT,
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            TRACE_ID_HEADER.clone(),
        ])
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .expose_headers([TRACE_ID_HEADER.clone()])
}

async fn require_api_auth(request: axum::extract::Request, next: Next) -> Response {
    let expected = configured_api_token();
    if !request_origin_is_allowed(request.headers(), expected.is_some()) {
        return AppError::new(
            StatusCode::FORBIDDEN,
            "api_origin_forbidden",
            "request Host or Origin is not allowed",
        )
        .into_response();
    }
    if let Some(expected) = expected {
        let supplied = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .map(str::trim);
        if !supplied
            .is_some_and(|supplied| constant_time_eq(expected.as_bytes(), supplied.as_bytes()))
        {
            let mut response = AppError::new(
                StatusCode::UNAUTHORIZED,
                "api_auth_required",
                "a valid API bearer token is required",
            )
            .into_response();
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Bearer realm=\"rweb-clash\""),
            );
            return response;
        }
    }

    match validate_json_write_body(request).await {
        Ok(request) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

fn request_origin_is_allowed(headers: &HeaderMap, api_token_configured: bool) -> bool {
    if !api_token_configured && !request_host_is_loopback(headers) {
        return false;
    }
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    if allowed_origins().contains(origin) {
        return true;
    }
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Ok(origin) = reqwest::Url::parse(origin.to_str().unwrap_or_default()) else {
        return false;
    };
    let Ok(authority) = host.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    let default_port = match origin.scheme() {
        "http" => 80,
        "https" => 443,
        _ => return false,
    };
    origin
        .host_str()
        .is_some_and(|origin_host| origin_host.eq_ignore_ascii_case(authority.host()))
        && origin.port_or_known_default() == Some(authority.port_u16().unwrap_or(default_port))
}

fn request_host_is_loopback(headers: &HeaderMap) -> bool {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Ok(authority) = host.parse::<axum::http::uri::Authority>() else {
        return false;
    };
    let host = authority
        .host()
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or_else(|| authority.host());
    host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("localhost.")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

async fn validate_json_write_body(
    request: axum::extract::Request,
) -> Result<axum::extract::Request, AppError> {
    if !matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH
    ) {
        return Ok(request);
    }

    let has_json_content_type = request_has_json_content_type(request.headers());
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, MAX_API_BODY_BYTES).await.map_err(|_| {
        AppError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "api_body_too_large",
            format!("request body exceeds the {MAX_API_BODY_BYTES} byte limit"),
        )
    })?;
    if !body.is_empty() && !has_json_content_type {
        return Err(AppError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "json_content_type_required",
            "request body must use application/json",
        ));
    }
    Ok(axum::extract::Request::from_parts(parts, Body::from(body)))
}

fn request_has_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

fn configured_api_token() -> Option<String> {
    std::env::var("RWEB_CLASH_API_TOKEN")
        .ok()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

fn constant_time_eq(expected: &[u8], supplied: &[u8]) -> bool {
    let max_len = expected.len().max(supplied.len());
    let mut difference = expected.len() ^ supplied.len();
    for index in 0..max_len {
        difference |= usize::from(
            expected.get(index).copied().unwrap_or_default()
                ^ supplied.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

fn allowed_origins() -> Vec<HeaderValue> {
    let mut origins = DEFAULT_ALLOWED_ORIGINS
        .iter()
        .map(|origin| HeaderValue::from_static(origin))
        .collect::<Vec<_>>();
    if let Ok(configured) = std::env::var("RWEB_CLASH_ALLOWED_ORIGINS") {
        for origin in configured
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            match HeaderValue::from_str(origin) {
                Ok(origin) if !origins.contains(&origin) => origins.push(origin),
                Ok(_) => {}
                Err(err) => warn!(%origin, %err, "ignoring invalid configured CORS origin"),
            }
        }
    }
    origins
}

#[cfg(test)]
mod cors_tests {
    use super::*;

    #[test]
    fn cors_defaults_are_explicit_local_application_origins() {
        assert!(DEFAULT_ALLOWED_ORIGINS.contains(&"tauri://localhost"));
        assert!(DEFAULT_ALLOWED_ORIGINS.contains(&"http://localhost:5173"));
        assert!(!DEFAULT_ALLOWED_ORIGINS.contains(&"*"));
    }

    #[test]
    fn bearer_tokens_are_compared_without_prefix_matches() {
        assert!(constant_time_eq(b"0123456789abcdef", b"0123456789abcdef"));
        assert!(!constant_time_eq(b"0123456789abcdef", b"0123456789abcde"));
        assert!(!constant_time_eq(b"0123456789abcdef", b"0123456789abcdef0"));
        assert!(!constant_time_eq(b"0123456789abcdef", b"xxxxxxxxxxxxxxxx"));
    }

    #[test]
    fn request_origins_allow_same_host_and_reject_foreign_sites() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:31990"));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://127.0.0.1:31990"),
        );
        assert!(request_origin_is_allowed(&headers, false));

        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://attacker.example"),
        );
        assert!(!request_origin_is_allowed(&headers, false));
    }

    #[test]
    fn tokenless_requests_reject_an_attacker_controlled_host_and_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("evil.example"));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://evil.example"),
        );

        assert!(!request_origin_is_allowed(&headers, false));
        assert!(request_origin_is_allowed(&headers, true));
    }

    #[test]
    fn tokenless_requests_reject_an_attacker_host_without_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("evil.example"));

        assert!(!request_origin_is_allowed(&headers, false));
        assert!(request_origin_is_allowed(&headers, true));
    }

    #[test]
    fn tokenless_requests_accept_localhost_and_ip_loopback_hosts() {
        for host in ["localhost:31990", "127.0.0.1:31990", "[::1]:31990"] {
            let mut headers = HeaderMap::new();
            headers.insert(header::HOST, HeaderValue::from_str(host).unwrap());
            assert!(request_origin_is_allowed(&headers, false), "{host}");
        }
    }

    #[tokio::test]
    async fn write_body_without_length_headers_still_requires_json() {
        let request = axum::extract::Request::builder()
            .method(Method::POST)
            .body(Body::from("{}"))
            .unwrap();

        let error = validate_json_write_body(request).await.unwrap_err();
        assert_eq!(error.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(error.code, "json_content_type_required");
    }

    #[tokio::test]
    async fn bodyless_commands_do_not_require_a_content_type() {
        let request = axum::extract::Request::builder()
            .method(Method::POST)
            .body(Body::empty())
            .unwrap();

        assert!(validate_json_write_body(request).await.is_ok());
    }

    #[tokio::test]
    async fn json_write_body_is_preserved_for_the_handler() {
        let request = axum::extract::Request::builder()
            .method(Method::PATCH)
            .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from("{\"mode\":\"rule\"}"))
            .unwrap();

        let request = validate_json_write_body(request).await.unwrap();
        let body = to_bytes(request.into_body(), MAX_API_BODY_BYTES)
            .await
            .unwrap();
        assert_eq!(body, r#"{"mode":"rule"}"#);
    }
}

async fn embedded_frontend(
    State(app): State<App>,
    uri: axum::http::Uri,
) -> Result<impl IntoResponse, StatusCode> {
    let Some(assets) = app.embedded_assets() else {
        return Err(StatusCode::NOT_FOUND);
    };
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let (bytes, content_type) =
        embedded_frontend_asset(assets, path).ok_or(StatusCode::NOT_FOUND)?;
    Ok((
        [(header::CONTENT_TYPE, HeaderValue::from_static(content_type))],
        bytes,
    ))
}

fn embedded_frontend_asset(
    assets: &'static crate::EmbeddedAssets,
    path: &str,
) -> Option<(&'static [u8], &'static str)> {
    let asset_path = format!("web/{path}");
    if let Some(bytes) = assets.get(&asset_path) {
        return Some((bytes, content_type(path)));
    }
    assets
        .get("web/index.html")
        .map(|bytes| (bytes, content_type("index.html")))
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod embedded_frontend_tests {
    use super::embedded_frontend_asset;
    use crate::{EmbeddedAssets, EmbeddedFile};

    static FILES: &[EmbeddedFile] = &[
        EmbeddedFile {
            path: "web/index.html",
            bytes: b"<html></html>",
        },
        EmbeddedFile {
            path: "web/assets/app.js",
            bytes: b"console.log('app')",
        },
    ];
    static ASSETS: EmbeddedAssets = EmbeddedAssets { files: FILES };

    #[test]
    fn frontend_routes_fall_back_to_html_with_an_html_content_type() {
        let (bytes, content_type) =
            embedded_frontend_asset(&ASSETS, "proxies").expect("serve frontend route fallback");
        assert_eq!(bytes, b"<html></html>");
        assert_eq!(content_type, "text/html; charset=utf-8");
    }

    #[test]
    fn embedded_static_assets_keep_their_own_content_type() {
        let (bytes, content_type) =
            embedded_frontend_asset(&ASSETS, "assets/app.js").expect("serve embedded javascript");
        assert_eq!(bytes, b"console.log('app')");
        assert_eq!(content_type, "text/javascript; charset=utf-8");
    }
}

#[cfg(test)]
mod openapi_tests {
    #[test]
    fn checked_in_openapi_document_is_valid_yaml() {
        serde_yaml::from_str::<serde_yaml::Value>(include_str!("../../../web/doc/openapi.yaml"))
            .expect("parse web OpenAPI document");
    }
}

async fn trace_request(mut request: axum::extract::Request, next: Next) -> Response {
    let trace_id = request
        .headers()
        .get(&TRACE_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let version = format!("{:?}", request.version());
    let user_agent = request_header(request.headers(), header::USER_AGENT);
    let request_bytes = content_length(request.headers());
    let started = Instant::now();
    request.extensions_mut().insert(trace_id.clone());

    let span = info_span!("api_request", trace_id = %trace_id, %method, path = %path);
    let scoped_trace_id = trace_id.clone();
    scope_trace_id(
        scoped_trace_id,
        async move {
            info!(
                trace_id = %trace_id,
                %method,
                path = %path,
                version = %version,
                user_agent = user_agent.as_deref().unwrap_or("-"),
                request_bytes,
                "api request received"
            );

            let mut response = next.run(request).await;
            let status = response.status();
            let response_bytes = content_length(response.headers());
            let elapsed_ms = started.elapsed().as_millis();
            if let Ok(value) = HeaderValue::from_str(&trace_id) {
                response.headers_mut().insert(&TRACE_ID_HEADER, value);
            }

            log_response(
                &trace_id,
                &method,
                &path,
                status,
                elapsed_ms,
                response_bytes,
            );
            response
        }
        .instrument(span),
    )
    .await
}

fn log_response(
    trace_id: &str,
    method: &axum::http::Method,
    path: &str,
    status: StatusCode,
    elapsed_ms: u128,
    response_bytes: Option<u64>,
) {
    if status.is_server_error() {
        error!(
            trace_id = %trace_id,
            %method,
            path = %path,
            status = status.as_u16(),
            elapsed_ms,
            response_bytes,
            "api response failed"
        );
    } else if status.is_client_error() {
        warn!(
            trace_id = %trace_id,
            %method,
            path = %path,
            status = status.as_u16(),
            elapsed_ms,
            response_bytes,
            "api response rejected"
        );
    } else {
        info!(
            trace_id = %trace_id,
            %method,
            path = %path,
            status = status.as_u16(),
            elapsed_ms,
            response_bytes,
            "api response completed"
        );
    }
}

fn request_header(headers: &HeaderMap, name: HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
}

async fn get_configs(State(app): State<App>) -> Result<Json<crate::types::SystemConfig>, AppError> {
    Ok(Json(app.config().await?))
}

async fn patch_configs(
    State(app): State<App>,
    body: Bytes,
) -> Result<Json<crate::types::SystemConfig>, AppError> {
    let patch = decode_body::<SystemConfigPatch>(body)?;
    Ok(Json(app.update_config(patch).await?))
}

async fn system_status(
    State(app): State<App>,
) -> Result<Json<crate::types::SystemStatusResponse>, AppError> {
    Ok(Json(app.system_status().await?))
}

async fn setup_status(
    State(app): State<App>,
) -> Result<Json<crate::types::SetupStatusResponse>, AppError> {
    Ok(Json(app.setup_status().await?))
}

async fn system_egress(
    State(app): State<App>,
) -> Result<Json<crate::types::EgressResponse>, AppError> {
    Ok(Json(app.egress().await?))
}

async fn core_status(
    State(app): State<App>,
) -> Result<Json<crate::types::CoreStatusResponse>, AppError> {
    Ok(Json(app.core_status().await?))
}

async fn core_start(
    State(app): State<App>,
) -> Result<Json<crate::types::CoreStatusResponse>, AppError> {
    Ok(Json(app.start_core().await?))
}

async fn core_stop(
    State(app): State<App>,
) -> Result<Json<crate::types::CoreStatusResponse>, AppError> {
    Ok(Json(app.stop_core().await?))
}

async fn core_restart(
    State(app): State<App>,
) -> Result<Json<crate::types::CoreStatusResponse>, AppError> {
    Ok(Json(app.restart_core().await?))
}

async fn list_subscriptions(
    State(app): State<App>,
) -> Result<Json<Vec<crate::types::SubscriptionResponse>>, AppError> {
    Ok(Json(app.list_subscriptions().await?))
}

async fn subscription_members(
    Path(id): Path<String>,
    State(app): State<App>,
) -> Result<Json<crate::types::SubscriptionMembersResponse>, AppError> {
    Ok(Json(app.subscription_members(&id).await?))
}

async fn create_subscription(
    State(app): State<App>,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let input = decode_body::<SubscriptionInput>(body)?;
    let list = app.create_subscription(input).await?;
    Ok((StatusCode::CREATED, Json(list)))
}

async fn update_subscription(
    Path(id): Path<String>,
    State(app): State<App>,
    body: Bytes,
) -> Result<Json<Vec<crate::types::SubscriptionResponse>>, AppError> {
    let input = decode_body::<SubscriptionInput>(body)?;
    Ok(Json(app.update_subscription(&id, input).await?))
}

async fn delete_subscription(
    Path(id): Path<String>,
    State(app): State<App>,
) -> Result<StatusCode, AppError> {
    app.delete_subscription(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn refresh_subscription(
    Path(id): Path<String>,
    State(app): State<App>,
) -> Result<Json<OperationResponse>, AppError> {
    app.refresh_subscription(&id).await?;
    Ok(Json(OperationResponse::ok("subscription refreshed")))
}

async fn global_filter_rules(
    State(app): State<App>,
) -> Result<Json<Vec<crate::types::FilterRule>>, AppError> {
    Ok(Json(app.global_filter_rules().await?))
}

async fn replace_global_filter_rules(
    State(app): State<App>,
    body: Bytes,
) -> Result<Json<Vec<crate::types::FilterRule>>, AppError> {
    let rules = decode_body::<Vec<FilterRuleInput>>(body)?;
    Ok(Json(app.replace_global_filter_rules(rules).await?))
}

async fn proxy_topology(
    State(app): State<App>,
) -> Result<Json<crate::types::ProxyTopologyResponse>, AppError> {
    Ok(Json(app.proxy_topology().await?))
}

async fn create_proxy_group(State(app): State<App>, body: Bytes) -> Result<StatusCode, AppError> {
    let input = decode_body::<ProxyGroupRequest>(body)?;
    app.create_proxy_group(input).await?;
    Ok(StatusCode::CREATED)
}

async fn update_or_select_proxy(
    Path(group): Path<String>,
    State(app): State<App>,
    body: Bytes,
) -> Result<Json<OperationResponse>, AppError> {
    let value = decode_body::<serde_json::Value>(body)?;
    if value.get("filter").is_some()
        || value.get("type").is_some()
        || value.get("groupType").is_some()
    {
        let mut request: ProxyGroupRequest = serde_json::from_value(value)?;
        if request.name.trim().is_empty() {
            request.name = group.clone();
        }
        app.update_proxy_group(&group, request).await?;
        return Ok(Json(OperationResponse::ok("proxy group updated")));
    }
    let request: SelectProxyRequest = serde_json::from_value(value)?;
    Ok(Json(app.select_proxy(&group, request).await?))
}

async fn delete_proxy_group(
    Path(group): Path<String>,
    State(app): State<App>,
) -> Result<StatusCode, AppError> {
    app.delete_proxy_group(&group).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn test_proxy_group(
    Path(group): Path<String>,
    State(app): State<App>,
) -> Result<Json<Vec<crate::types::DelayResponse>>, AppError> {
    Ok(Json(app.test_group(&group).await?))
}

async fn test_node(
    State(app): State<App>,
    body: Bytes,
) -> Result<Json<crate::types::DelayResponse>, AppError> {
    let request = decode_body::<SelectProxyRequest>(body)?;
    Ok(Json(app.test_node(&request.name).await?))
}

async fn list_manual_nodes(
    State(app): State<App>,
) -> Result<Json<Vec<crate::types::ManualNodeResponse>>, AppError> {
    Ok(Json(app.manual_nodes().await?))
}

async fn create_manual_node(
    State(app): State<App>,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let input = decode_body::<ManualNodeInput>(body)?;
    Ok((
        StatusCode::CREATED,
        Json(app.create_manual_node(input).await?),
    ))
}

async fn update_manual_node(
    Path(name): Path<String>,
    State(app): State<App>,
    body: Bytes,
) -> Result<Json<Vec<crate::types::ManualNodeResponse>>, AppError> {
    let input = decode_body::<ManualNodeInput>(body)?;
    Ok(Json(app.update_manual_node(&name, input).await?))
}

async fn delete_manual_node(
    Path(name): Path<String>,
    State(app): State<App>,
) -> Result<StatusCode, AppError> {
    app.delete_manual_node(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_rules(
    State(app): State<App>,
) -> Result<Json<Vec<crate::types::RuleResponse>>, AppError> {
    Ok(Json(app.list_rules().await?))
}

async fn create_rule(State(app): State<App>, body: Bytes) -> Result<impl IntoResponse, AppError> {
    let input = decode_body::<RuleInput>(body)?;
    Ok((StatusCode::CREATED, Json(app.create_rule(input).await?)))
}

async fn update_rule(
    Path(id): Path<String>,
    State(app): State<App>,
    body: Bytes,
) -> Result<Json<crate::types::RuleResponse>, AppError> {
    let input = decode_body::<RuleInput>(body)?;
    Ok(Json(app.update_rule(&id, input).await?))
}

async fn delete_rule(
    Path(id): Path<String>,
    State(app): State<App>,
) -> Result<StatusCode, AppError> {
    app.delete_rule(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn test_rule(
    State(app): State<App>,
    body: Bytes,
) -> Result<Json<crate::types::RuleTestResponse>, AppError> {
    let input = decode_body::<RuleTestRequest>(body)?;
    Ok(Json(app.test_rule(input).await?))
}

async fn list_rule_sets(
    State(app): State<App>,
) -> Result<Json<Vec<crate::types::RuleSetResponse>>, AppError> {
    Ok(Json(app.list_rule_sets().await?))
}

async fn create_rule_set(
    State(app): State<App>,
    body: Bytes,
) -> Result<impl IntoResponse, AppError> {
    let input = decode_body::<RuleSetInput>(body)?;
    Ok((StatusCode::CREATED, Json(app.create_rule_set(input).await?)))
}

async fn refresh_rule_set(
    Path(id): Path<String>,
    State(app): State<App>,
) -> Result<Json<OperationResponse>, AppError> {
    app.refresh_rule_set(&id).await?;
    Ok(Json(OperationResponse::ok("rule set refreshed")))
}

async fn delete_rule_set(
    Path(id): Path<String>,
    State(app): State<App>,
) -> Result<StatusCode, AppError> {
    app.delete_rule_set(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    level: Option<String>,
    search: Option<String>,
}

async fn list_logs(
    Query(query): Query<LogsQuery>,
    State(app): State<App>,
) -> Result<Json<Vec<crate::types::LogEntryResponse>>, AppError> {
    Ok(Json(
        app.logs(query.level.as_deref(), query.search.as_deref())
            .await?,
    ))
}

async fn clear_logs(State(app): State<App>) -> Result<StatusCode, AppError> {
    app.clear_logs().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn export_logs(State(app): State<App>) -> Result<String, AppError> {
    app.export_logs().await
}

async fn export_diagnostics(State(app): State<App>) -> Result<String, AppError> {
    app.export_diagnostics().await
}

async fn list_backups(
    State(app): State<App>,
) -> Result<Json<Vec<crate::types::BackupResponse>>, AppError> {
    Ok(Json(app.backups().await?))
}

async fn create_backup(State(app): State<App>) -> Result<impl IntoResponse, AppError> {
    Ok((StatusCode::CREATED, Json(app.create_backup().await?)))
}

async fn delete_backup(
    Path(name): Path<String>,
    State(app): State<App>,
) -> Result<StatusCode, AppError> {
    app.delete_backup(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn restore_backup(
    Path(name): Path<String>,
    State(app): State<App>,
) -> Result<Json<OperationResponse>, AppError> {
    app.restore_backup(&name).await?;
    Ok(Json(OperationResponse::ok("backup restored")))
}

async fn webdav_settings(
    State(app): State<App>,
) -> Result<Json<crate::types::WebDavSettingsResponse>, AppError> {
    Ok(Json(app.webdav_settings().await?))
}

async fn save_webdav_settings(
    State(app): State<App>,
    body: Bytes,
) -> Result<Json<crate::types::WebDavSettingsResponse>, AppError> {
    let input = decode_body::<WebDavSettingsInput>(body)?;
    Ok(Json(app.save_webdav_settings(input).await?))
}

async fn test_webdav(State(app): State<App>) -> Result<Json<OperationResponse>, AppError> {
    app.test_webdav().await?;
    Ok(Json(OperationResponse::ok("WebDAV connection succeeded")))
}

async fn sync_webdav(
    State(app): State<App>,
) -> Result<Json<crate::types::BackupResponse>, AppError> {
    Ok(Json(app.sync_webdav().await?))
}

async fn restore_webdav(State(app): State<App>) -> Result<Json<OperationResponse>, AppError> {
    app.restore_webdav().await?;
    Ok(Json(OperationResponse::ok("WebDAV backup restored")))
}

async fn traffic(State(app): State<App>) -> Json<crate::types::TrafficResponse> {
    Json(app.traffic().await)
}

async fn connections(State(app): State<App>) -> Json<Vec<crate::types::ConnectionResponse>> {
    Json(app.connections().await)
}

async fn close_connection(
    Path(id): Path<String>,
    State(app): State<App>,
) -> Result<StatusCode, AppError> {
    app.close_connection(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn close_all_connections(State(app): State<App>) -> Result<StatusCode, AppError> {
    app.close_all_connections().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn flush_dns(State(app): State<App>) -> Result<StatusCode, AppError> {
    app.flush_dns().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn decode_body<T>(body: Bytes) -> Result<T, AppError>
where
    T: DeserializeOwned,
{
    if body.is_empty() {
        return Err(AppError::bad_request(
            "invalid_json",
            "request body is empty",
        ));
    }
    serde_json::from_slice::<T>(&body).map_err(AppError::from)
}
