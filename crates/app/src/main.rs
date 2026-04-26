use async_stream::stream;
use axum::extract::Extension;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive};
use axum::response::{Html, IntoResponse, Json, Response, Sse};
use axum::routing::{delete, get, post, put};
use axum::Router;
use controller_client::ControllerClient;
use core_manager::{CoreManager, CoreStartConfig};
use platform_linux::AppPaths;
use profile::ProfileStore;
use shared_types::{
    ApiErrorResponse, ConnectionSummary, CoreRunState, CoreStatusResponse, ImportFileRequest,
    ImportUrlRequest, OperationResponse, ProfileDetailResponse, ProfilePreviewResponse,
    ProfileSummary, ProxyDelayResponse, ProxyGroupSummary, ScriptDetailResponse, ScriptSummary,
    SelectProxyRequest, ServerEvent, SystemConfigResponse, SystemInfoResponse,
    UpdateProfileRequest, UpdateScriptRequest, UpdateSystemConfigRequest, UpsertScriptRequest,
    MERGED_PROFILE_ID,
};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tracing::{info, warn, Instrument};
use uuid::Uuid;

static TRACE_ID_HEADER: HeaderName = HeaderName::from_static("x-trace-id");
const CORE_START_READY_TIMEOUT: Duration = Duration::from_secs(5);
const CORE_START_STABLE_WINDOW: Duration = Duration::from_millis(800);
const CORE_START_POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_DELAY_TEST_URL: &str = "http://www.gstatic.com/generate_204";
const DEFAULT_DELAY_TEST_TIMEOUT_MS: u64 = 5000;

#[derive(Debug, Clone)]
struct TraceId(String);

#[derive(Clone)]
struct AppState {
    paths: AppPaths,
    profiles: ProfileStore,
    core: CoreManager,
    events: broadcast::Sender<ServerEvent>,
}

impl AppState {
    async fn controller_client(&self) -> ControllerClient {
        let snapshot = self.profiles.snapshot().await;
        let secret = if snapshot.controller_secret.is_empty() {
            None
        } else {
            Some(snapshot.controller_secret)
        };
        ControllerClient::new(snapshot.controller_addr, secret)
    }

    async fn system_info(&self) -> SystemInfoResponse {
        let snapshot = self.profiles.snapshot().await;
        let bundled_path = self.paths.bundled_mihomo_binary();
        SystemInfoResponse {
            platform: std::env::consts::OS.to_string(),
            app_dir: AppPaths::display_path(&self.paths.app_dir),
            runtime_config: AppPaths::display_path(&self.paths.runtime_config),
            api_addr: snapshot.listen_addr,
            controller_addr: snapshot.controller_addr,
            mihomo_expected_path: AppPaths::display_path(&bundled_path),
            mihomo_path: self
                .paths
                .resolve_mihomo_binary()
                .map(|path| AppPaths::display_path(&path)),
            active_profile_id: snapshot.active_profile_id,
        }
    }

    async fn current_status(&self) -> Result<CoreStatusResponse, ApiError> {
        let snapshot = self.profiles.snapshot().await;
        self.core
            .snapshot(snapshot.active_profile_id, snapshot.controller_addr)
            .await
            .map_err(ApiError::internal)
    }

    async fn build_start_config(&self) -> Result<CoreStartConfig, ApiError> {
        let snapshot = self.profiles.snapshot().await;
        let runtime_config = self
            .profiles
            .ensure_active_runtime_config()
            .await
            .map_err(ApiError::bad_request)?;
        let mihomo_binary = self
            .paths
            .resolve_mihomo_binary()
            .ok_or_else(|| {
                ApiError::bad_request(
                    "bundled mihomo binary not found; place mihomo in ./cache-core before starting the core",
                )
            })?;

        Ok(CoreStartConfig {
            active_profile_id: snapshot.active_profile_id,
            controller_addr: snapshot.controller_addr,
            mihomo_binary,
            runtime_config,
            runtime_dir: self.paths.runtime_dir.clone(),
        })
    }

    async fn publish_profiles(&self) {
        let _ = self
            .events
            .send(ServerEvent::Profiles(self.profiles.list_profiles().await));
    }

    async fn refresh_active_profile_if_using_script(
        &self,
        script_id: &str,
    ) -> Result<(), ApiError> {
        let snapshot = self.profiles.snapshot().await;
        let Some(active_profile_id) = snapshot.active_profile_id else {
            return Ok(());
        };
        if active_profile_id == MERGED_PROFILE_ID {
            let uses_script = snapshot
                .profiles
                .iter()
                .any(|profile| profile.script_id.as_deref() == Some(script_id));
            if uses_script {
                self.profiles
                    .refresh_profile(MERGED_PROFILE_ID)
                    .await
                    .map_err(ApiError::bad_request)?;
                self.reload_runtime_if_running().await?;
            }
            return Ok(());
        }
        let Some(profile) = snapshot
            .profiles
            .iter()
            .find(|profile| profile.id == active_profile_id)
        else {
            return Ok(());
        };
        if profile.script_id.as_deref() != Some(script_id) {
            return Ok(());
        }

        self.profiles
            .refresh_profile(&active_profile_id)
            .await
            .map_err(ApiError::bad_request)?;
        self.reload_runtime_if_running().await
    }

    async fn publish_status(&self) {
        if let Ok(status) = self.current_status().await {
            let _ = self.events.send(ServerEvent::CoreStatus(status));
        }
    }

    async fn reload_runtime_if_running(&self) -> Result<(), ApiError> {
        if !self.core.is_running().await.map_err(ApiError::internal)? {
            return Ok(());
        }

        let start = self.build_start_config().await?;
        let runtime_path = AppPaths::display_path(&start.runtime_config);
        let controller = self.controller_client().await;

        if let Err(err) = controller.reload_config(&runtime_path).await {
            warn!("controller reload failed, restarting mihomo instead: {err}");
            self.core.restart(start).await.map_err(ApiError::internal)?;
        }

        self.publish_status().await;
        Ok(())
    }

    async fn active_profile_is_merged(&self) -> bool {
        self.profiles.snapshot().await.active_profile_id.as_deref() == Some(MERGED_PROFILE_ID)
    }

    async fn ensure_controller_running(&self) -> Result<(), ApiError> {
        let status = self.current_status().await?;
        if matches!(status.state, CoreRunState::Running) {
            Ok(())
        } else {
            Err(ApiError::service_unavailable(
                "mihomo core is not running; start the core before querying controller data",
            ))
        }
    }

    async fn wait_for_controller_ready_after_start(&self) -> Result<(), ApiError> {
        let deadline = Instant::now() + CORE_START_READY_TIMEOUT;
        let mut ready_since = None;
        let mut last_controller_error = None;

        loop {
            let status = self.current_status().await?;
            if !matches!(status.state, CoreRunState::Running) {
                return Err(ApiError::service_unavailable(
                    status.last_error.unwrap_or_else(|| {
                        "mihomo exited before the controller became ready".into()
                    }),
                ));
            }

            let controller = self.controller_client().await;
            match controller.version().await {
                Ok(_) => {
                    let ready_at = ready_since.get_or_insert_with(Instant::now);
                    if ready_at.elapsed() >= CORE_START_STABLE_WINDOW {
                        return Ok(());
                    }
                }
                Err(err) => {
                    ready_since = None;
                    last_controller_error = Some(err.to_string());
                }
            }

            if Instant::now() >= deadline {
                let suffix = last_controller_error
                    .map(|err| format!(": {err}"))
                    .unwrap_or_default();
                return Err(ApiError::service_unavailable(format!(
                    "mihomo controller did not become ready within {} seconds{suffix}",
                    CORE_START_READY_TIMEOUT.as_secs()
                )));
            }

            tokio::time::sleep(CORE_START_POLL_INTERVAL).await;
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rweb_clash=info,app=info,axum=info".into()),
        )
        .init();

    let (events, _) = broadcast::channel(512);
    let paths = AppPaths::discover()?;
    let profiles = ProfileStore::load(paths.clone())
        .await?
        .with_event_sender(events.clone());
    let core = CoreManager::new(paths.clone(), events.clone());
    let state = AppState {
        paths: paths.clone(),
        profiles,
        core,
        events,
    };

    let api = Router::new()
        .route("/api/system/info", get(get_system_info))
        .route(
            "/api/system/config",
            get(get_system_config).put(update_system_config),
        )
        .route("/api/core/status", get(get_core_status))
        .route("/api/core/start", post(start_core))
        .route("/api/core/stop", post(stop_core))
        .route("/api/core/restart", post(restart_core))
        .route("/api/scripts", get(list_scripts).post(create_script))
        .route(
            "/api/scripts/{id}",
            get(get_script).put(update_script).delete(delete_script),
        )
        .route("/api/profiles", get(list_profiles))
        .route("/api/profiles/{id}", get(get_profile).put(update_profile))
        .route("/api/profiles/{id}/preview", get(get_profile_preview))
        .route("/api/profiles/import-url", post(import_url))
        .route("/api/profiles/import-file", post(import_file))
        .route("/api/profiles/{id}/refresh", post(refresh_profile))
        .route("/api/profiles/{id}/activate", post(activate_profile))
        .route("/api/proxies", get(list_proxies))
        .route("/api/proxies/{name}/delay", get(test_proxy_delay))
        .route(
            "/api/proxy-groups/{name}/delay",
            get(test_proxy_group_delay),
        )
        .route("/api/proxies/{group}/select", put(select_proxy))
        .route("/api/connections", get(list_connections))
        .route("/api/connections/{id}", delete(close_connection))
        .route("/api/logs/stream", get(stream_events))
        .layer(middleware::from_fn(trace_request))
        .with_state(state.clone());

    let app = api
        .route("/", get(index))
        .nest_service("/assets", ServeDir::new(frontend_assets_dir()))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_headers(Any)
                .allow_methods(Any),
        );

    let system = state.system_info().await;
    let addr: SocketAddr = system
        .api_addr
        .parse()
        .map_err(|err| anyhow::anyhow!("invalid listen address {}: {err}", system.api_addr))?;

    state.publish_profiles().await;
    state.publish_status().await;
    let refresh_state = state.clone();
    tokio::spawn(async move {
        run_profile_refresh_scheduler(refresh_state).await;
    });

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("rweb-clash listening on http://{}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn index() -> Html<String> {
    let index_path = frontend_dist_dir().join("index.html");
    match tokio::fs::read_to_string(index_path).await {
        Ok(html) => Html(html),
        Err(_) => Html(
            r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>rweb-clash</title>
    <style>
      :root { color-scheme: light; }
      body {
        margin: 0;
        font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
        background: linear-gradient(135deg, #f7f2e8 0%, #ece7de 45%, #d7dfd6 100%);
        color: #1e2a26;
      }
      main {
        max-width: 780px;
        margin: 64px auto;
        padding: 32px;
        background: rgba(255, 255, 255, 0.78);
        border: 1px solid rgba(30, 42, 38, 0.12);
        border-radius: 28px;
        box-shadow: 0 24px 80px rgba(30, 42, 38, 0.12);
      }
      h1 { margin-top: 0; font-size: 2.2rem; }
      code {
        display: inline-block;
        padding: 0.2rem 0.45rem;
        border-radius: 0.5rem;
        background: rgba(30, 42, 38, 0.08);
      }
    </style>
  </head>
  <body>
    <main>
      <h1>rweb-clash backend is running</h1>
      <p>The Rust API is available, but the React frontend has not been built yet.</p>
      <p>From the repository root, run <code>pnpm install</code> and <code>pnpm dev</code> inside <code>web/</code>, or build it with <code>pnpm build</code>.</p>
    </main>
  </body>
</html>"#
                .to_string(),
        ),
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
    let uri = request.uri().clone();
    let started_at = std::time::Instant::now();

    request.extensions_mut().insert(TraceId(trace_id.clone()));

    let span = tracing::info_span!(
        "http_request",
        trace_id = %trace_id,
        method = %method,
        uri = %uri,
    );

    info!(
        trace_id = %trace_id,
        method = %method,
        uri = %uri,
        "request started"
    );

    let mut response = next.run(request).instrument(span).await;
    let status = response.status();
    let elapsed_ms = started_at.elapsed().as_millis();

    if let Ok(header_value) = HeaderValue::from_str(&trace_id) {
        response
            .headers_mut()
            .insert(&TRACE_ID_HEADER, header_value);
    }

    if status.is_server_error() {
        warn!(
            trace_id = %trace_id,
            method = %method,
            uri = %uri,
            status = %status,
            elapsed_ms,
            "request completed with server error"
        );
    } else {
        info!(
            trace_id = %trace_id,
            method = %method,
            uri = %uri,
            status = %status,
            elapsed_ms,
            "request completed"
        );
    }

    response
}

fn trace_value(trace_id: &TraceId) -> &str {
    trace_id.0.as_str()
}

async fn get_system_info(State(state): State<AppState>) -> Json<SystemInfoResponse> {
    Json(state.system_info().await)
}

async fn get_system_config(State(state): State<AppState>) -> Json<SystemConfigResponse> {
    Json(state.profiles.system_config().await)
}

async fn update_system_config(
    State(state): State<AppState>,
    Json(request): Json<UpdateSystemConfigRequest>,
) -> Result<Json<SystemConfigResponse>, ApiError> {
    let config = state
        .profiles
        .update_system_config(request)
        .await
        .map_err(ApiError::bad_request)?;
    state.reload_runtime_if_running().await?;
    state.publish_status().await;
    Ok(Json(config))
}

async fn get_core_status(
    State(state): State<AppState>,
) -> Result<Json<CoreStatusResponse>, ApiError> {
    Ok(Json(state.current_status().await?))
}

async fn start_core(
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
) -> Result<Json<CoreStatusResponse>, ApiError> {
    info!(trace_id = %trace_value(&trace_id), operation = "core.start", "operation started");
    info!(trace_id = %trace_value(&trace_id), operation = "core.start", stage = "build_start_config", "operation stage started");
    let start = state.build_start_config().await?;
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "core.start",
        stage = "build_start_config",
        runtime_config = %start.runtime_config.display(),
        mihomo_binary = %start.mihomo_binary.display(),
        "operation stage completed"
    );
    info!(trace_id = %trace_value(&trace_id), operation = "core.start", stage = "core_manager_start", "operation stage started");
    state.core.start(start).await.map_err(ApiError::internal)?;
    state.wait_for_controller_ready_after_start().await?;
    let status = state.current_status().await?;
    info!(trace_id = %trace_value(&trace_id), operation = "core.start", stage = "publish_status", "operation stage started");
    state.publish_status().await;
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "core.start",
        state = ?status.state,
        pid = ?status.pid,
        "operation completed"
    );
    Ok(Json(status))
}

async fn stop_core(
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
) -> Result<Json<CoreStatusResponse>, ApiError> {
    info!(trace_id = %trace_value(&trace_id), operation = "core.stop", "operation started");
    let snapshot = state.profiles.snapshot().await;
    let status = state
        .core
        .stop(snapshot.active_profile_id, snapshot.controller_addr)
        .await
        .map_err(ApiError::internal)?;
    state.publish_status().await;
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "core.stop",
        state = ?status.state,
        "operation completed"
    );
    Ok(Json(status))
}

async fn restart_core(
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
) -> Result<Json<CoreStatusResponse>, ApiError> {
    info!(trace_id = %trace_value(&trace_id), operation = "core.restart", "operation started");
    let start = state.build_start_config().await?;
    state
        .core
        .restart(start)
        .await
        .map_err(ApiError::internal)?;
    state.wait_for_controller_ready_after_start().await?;
    let status = state.current_status().await?;
    state.publish_status().await;
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "core.restart",
        state = ?status.state,
        pid = ?status.pid,
        "operation completed"
    );
    Ok(Json(status))
}

async fn list_scripts(State(state): State<AppState>) -> Json<Vec<ScriptSummary>> {
    Json(state.profiles.list_scripts().await)
}

async fn get_script(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<ScriptDetailResponse>, ApiError> {
    let detail = state
        .profiles
        .script_detail(&id)
        .await
        .map_err(ApiError::bad_request)?;
    Ok(Json(detail))
}

async fn create_script(
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
    Json(request): Json<UpsertScriptRequest>,
) -> Result<Json<ScriptSummary>, ApiError> {
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "script.create",
        script_name = %request.name,
        "operation started"
    );
    let summary = state
        .profiles
        .create_script(request)
        .await
        .map_err(ApiError::bad_request)?;
    state.publish_profiles().await;
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "script.create",
        script_id = %summary.id,
        "operation completed"
    );
    Ok(Json(summary))
}

async fn update_script(
    Path(id): Path<String>,
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
    Json(request): Json<UpdateScriptRequest>,
) -> Result<Json<ScriptSummary>, ApiError> {
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "script.update",
        script_id = %id,
        "operation started"
    );
    let summary = state
        .profiles
        .update_script(&id, request)
        .await
        .map_err(ApiError::bad_request)?;
    state.refresh_active_profile_if_using_script(&id).await?;
    state.publish_profiles().await;
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "script.update",
        script_id = %summary.id,
        "operation completed"
    );
    Ok(Json(summary))
}

async fn delete_script(
    Path(id): Path<String>,
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
) -> Result<Json<OperationResponse>, ApiError> {
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "script.delete",
        script_id = %id,
        "operation started"
    );
    let active_profile_id = {
        let snapshot = state.profiles.snapshot().await;
        snapshot.active_profile_id.and_then(|active_profile_id| {
            snapshot
                .profiles
                .iter()
                .find(|profile| profile.id == active_profile_id)
                .filter(|profile| profile.script_id.as_deref() == Some(id.as_str()))
                .map(|_| active_profile_id)
        })
    };

    state
        .profiles
        .delete_script(&id)
        .await
        .map_err(ApiError::bad_request)?;
    if let Some(active_profile_id) = active_profile_id {
        state
            .profiles
            .refresh_profile(&active_profile_id)
            .await
            .map_err(ApiError::bad_request)?;
        state.reload_runtime_if_running().await?;
    }
    state.publish_profiles().await;
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "script.delete",
        script_id = %id,
        "operation completed"
    );
    Ok(Json(OperationResponse::ok("script deleted")))
}

async fn list_profiles(State(state): State<AppState>) -> Json<Vec<ProfileSummary>> {
    Json(state.profiles.list_profiles().await)
}

async fn get_profile_preview(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<ProfilePreviewResponse>, ApiError> {
    let preview = state
        .profiles
        .preview_profile(&id)
        .await
        .map_err(ApiError::bad_request)?;
    Ok(Json(preview))
}

async fn get_profile(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<ProfileDetailResponse>, ApiError> {
    let detail = state
        .profiles
        .profile_detail(&id)
        .await
        .map_err(ApiError::bad_request)?;
    Ok(Json(detail))
}

async fn import_url(
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
    Json(request): Json<ImportUrlRequest>,
) -> Result<Json<ProfileSummary>, ApiError> {
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "profile.import_url",
        url = %request.url,
        "operation started"
    );
    let summary = state
        .profiles
        .import_url(request)
        .await
        .map_err(ApiError::bad_request)?;
    state.publish_profiles().await;
    if state.active_profile_is_merged().await {
        state.reload_runtime_if_running().await?;
    }
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "profile.import_url",
        profile_id = %summary.id,
        "operation completed"
    );
    Ok(Json(summary))
}

async fn import_file(
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
    Json(request): Json<ImportFileRequest>,
) -> Result<Json<ProfileSummary>, ApiError> {
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "profile.import_file",
        filename = ?request.filename,
        "operation started"
    );
    let summary = state
        .profiles
        .import_file(request)
        .await
        .map_err(ApiError::bad_request)?;
    state.publish_profiles().await;
    if state.active_profile_is_merged().await {
        state.reload_runtime_if_running().await?;
    }
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "profile.import_file",
        profile_id = %summary.id,
        "operation completed"
    );
    Ok(Json(summary))
}

async fn update_profile(
    Path(id): Path<String>,
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
    Json(request): Json<UpdateProfileRequest>,
) -> Result<Json<ProfileSummary>, ApiError> {
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "profile.update",
        profile_id = %id,
        "operation started"
    );
    let profile = state
        .profiles
        .update_profile(&id, request)
        .await
        .map_err(ApiError::bad_request)?;
    state.publish_profiles().await;
    if profile.active || state.active_profile_is_merged().await {
        state.reload_runtime_if_running().await?;
    }
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "profile.update",
        profile_id = %profile.id,
        active = profile.active,
        "operation completed"
    );
    Ok(Json(profile))
}

async fn refresh_profile(
    Path(id): Path<String>,
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
) -> Result<Json<ProfileSummary>, ApiError> {
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "profile.refresh",
        profile_id = %id,
        "operation started"
    );
    let summary = state
        .profiles
        .refresh_profile(&id)
        .await
        .map_err(ApiError::bad_request)?;
    state.publish_profiles().await;

    if summary.active || state.active_profile_is_merged().await {
        state.reload_runtime_if_running().await?;
    }

    info!(
        trace_id = %trace_value(&trace_id),
        operation = "profile.refresh",
        profile_id = %summary.id,
        active = summary.active,
        "operation completed"
    );
    Ok(Json(summary))
}

async fn activate_profile(
    Path(id): Path<String>,
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
) -> Result<Json<OperationResponse>, ApiError> {
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "profile.activate",
        profile_id = %id,
        "operation started"
    );
    state
        .profiles
        .activate_profile(&id)
        .await
        .map_err(ApiError::bad_request)?;
    state.publish_profiles().await;
    state.reload_runtime_if_running().await?;
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "profile.activate",
        profile_id = %id,
        "operation completed"
    );
    Ok(Json(OperationResponse::ok("profile activated")))
}

async fn list_proxies(
    State(state): State<AppState>,
) -> Result<Json<Vec<ProxyGroupSummary>>, ApiError> {
    state.ensure_controller_running().await?;
    let controller = state.controller_client().await;
    let proxies = controller.proxies().await.map_err(map_controller_error)?;
    Ok(Json(proxies))
}

#[derive(Debug, serde::Deserialize)]
struct ProxyDelayQuery {
    url: Option<String>,
    timeout: Option<u64>,
}

async fn test_proxy_delay(
    Path(name): Path<String>,
    Query(query): Query<ProxyDelayQuery>,
    State(state): State<AppState>,
) -> Result<Json<ProxyDelayResponse>, ApiError> {
    state.ensure_controller_running().await?;
    let url = query
        .url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or(DEFAULT_DELAY_TEST_URL);
    let timeout = query.timeout.unwrap_or(DEFAULT_DELAY_TEST_TIMEOUT_MS);
    let controller = state.controller_client().await;
    let delay = controller
        .proxy_delay(&name, url, timeout)
        .await
        .map_err(map_controller_error)?;
    Ok(Json(delay))
}

async fn test_proxy_group_delay(
    Path(name): Path<String>,
    Query(query): Query<ProxyDelayQuery>,
    State(state): State<AppState>,
) -> Result<Json<Vec<ProxyDelayResponse>>, ApiError> {
    state.ensure_controller_running().await?;
    let url = query
        .url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or(DEFAULT_DELAY_TEST_URL);
    let timeout = query.timeout.unwrap_or(DEFAULT_DELAY_TEST_TIMEOUT_MS);
    let controller = state.controller_client().await;
    let delays = controller
        .group_delay(&name, url, timeout)
        .await
        .map_err(map_controller_error)?;
    Ok(Json(delays))
}

async fn select_proxy(
    Path(group): Path<String>,
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
    Json(request): Json<SelectProxyRequest>,
) -> Result<Json<OperationResponse>, ApiError> {
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "proxy.select",
        group = %group,
        proxy = %request.name,
        "operation started"
    );
    state.ensure_controller_running().await?;
    let controller = state.controller_client().await;
    controller
        .select_proxy(&group, &request.name)
        .await
        .map_err(map_controller_error)?;
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "proxy.select",
        group = %group,
        proxy = %request.name,
        "operation completed"
    );
    Ok(Json(OperationResponse::ok("proxy updated")))
}

async fn list_connections(
    State(state): State<AppState>,
) -> Result<Json<Vec<ConnectionSummary>>, ApiError> {
    state.ensure_controller_running().await?;
    let controller = state.controller_client().await;
    let connections = controller
        .connections()
        .await
        .map_err(map_controller_error)?;
    Ok(Json(connections))
}

async fn close_connection(
    Path(id): Path<String>,
    Extension(trace_id): Extension<TraceId>,
    State(state): State<AppState>,
) -> Result<Json<OperationResponse>, ApiError> {
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "connection.close",
        connection_id = %id,
        "operation started"
    );
    state.ensure_controller_running().await?;
    let controller = state.controller_client().await;
    controller
        .close_connection(&id)
        .await
        .map_err(map_controller_error)?;
    info!(
        trace_id = %trace_value(&trace_id),
        operation = "connection.close",
        connection_id = %id,
        "operation completed"
    );
    Ok(Json(OperationResponse::ok("connection closed")))
}

async fn stream_events(
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.events.subscribe();
    let event_stream = stream! {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    if let Ok(data) = serde_json::to_string(&event) {
                        yield Ok(Event::default().event(event.event_name()).data(data));
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(event_stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(ApiErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

fn frontend_dist_dir() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("web")
        .join("dist")
}

fn frontend_assets_dir() -> PathBuf {
    frontend_dist_dir().join("assets")
}

fn map_controller_error(error: controller_client::ControllerError) -> ApiError {
    match error {
        controller_client::ControllerError::Http(_) => ApiError::service_unavailable(
            "controller is unreachable; verify mihomo is running and external-controller matches the app config",
        ),
        controller_client::ControllerError::Decode(error) => ApiError {
            status: StatusCode::BAD_GATEWAY,
            message: format!("controller returned an unsupported response shape: {error}"),
        },
        controller_client::ControllerError::UnexpectedStatus(status) => ApiError {
            status: StatusCode::BAD_GATEWAY,
            message: format!("controller returned unexpected status {status}"),
        },
    }
}

async fn run_profile_refresh_scheduler(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        let due_profile_ids = state.profiles.due_profile_ids().await;
        for profile_id in due_profile_ids {
            info!(
                operation = "profile.auto_refresh",
                profile_id = %profile_id,
                "operation started"
            );
            match state.profiles.refresh_profile(&profile_id).await {
                Ok(summary) => {
                    state.publish_profiles().await;
                    if summary.active || state.active_profile_is_merged().await {
                        if let Err(err) = state.reload_runtime_if_running().await {
                            warn!(
                                operation = "profile.auto_refresh",
                                profile_id = %profile_id,
                                "failed to reload active refreshed profile: {}",
                                err.message
                            );
                        }
                    }
                    info!(
                        operation = "profile.auto_refresh",
                        profile_id = %profile_id,
                        "operation completed"
                    );
                }
                Err(err) => warn!(
                    operation = "profile.auto_refresh",
                    profile_id = %profile_id,
                    "operation failed: {err}"
                ),
            }
        }
    }
}
