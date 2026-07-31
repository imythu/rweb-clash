use crate::error::AppError;
use crate::paths::AppPaths;
use crate::storage::Storage;
use crate::types::CoreStatusResponse;
#[cfg(any(target_os = "macos", test))]
use crate::util::content_hash;
use crate::util::{now_iso, parse_host_from_log};
use axum::http::StatusCode;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info, warn};

#[cfg(any(target_os = "macos", test))]
use tokio::io::AsyncReadExt;
#[cfg(any(target_os = "macos", test))]
use tokio::net::tcp::OwnedWriteHalf;
#[cfg(any(target_os = "macos", test))]
use tokio::net::TcpListener;
#[cfg(any(target_os = "macos", test))]
use tokio::net::UnixStream;

#[cfg(any(target_os = "macos", test))]
const MACOS_HELPER_SOCKET: &str = "/var/run/rweb-clash-tun.sock";

#[cfg(any(target_os = "macos", test))]
#[derive(serde::Deserialize)]
struct MacosHelperResponse {
    ok: bool,
    error: Option<String>,
    pid: Option<u32>,
}

const DEFAULT_MIHOMO_VALIDATION_TIMEOUT_SECS: u64 = 120;
const MAX_MIHOMO_VALIDATION_TIMEOUT_SECS: u64 = 3_600;
const MIHOMO_VALIDATION_TIMEOUT_ENV: &str = "RWEB_CLASH_MIHOMO_VALIDATION_TIMEOUT_SECS";
const CORE_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const CORE_STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(200);
#[cfg(any(target_os = "macos", test))]
const MACOS_TUN_AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(120);

fn mihomo_command(binary: &std::path::Path) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new(binary);
        command.creation_flags(0x0800_0000);
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new(binary)
    }
}

#[cfg(any(target_os = "macos", test))]
async fn macos_helper_request(request: serde_json::Value) -> Result<MacosHelperResponse, AppError> {
    use tokio::io::AsyncWriteExt;
    let mut stream = UnixStream::connect(MACOS_HELPER_SOCKET)
        .await
        .map_err(|error| {
            AppError::service_unavailable(
                "tun_helper_unavailable",
                format!("macOS privileged helper is unavailable: {error}"),
            )
        })?;
    let mut payload = serde_json::to_vec(&request).map_err(AppError::internal)?;
    payload.push(b'\n');
    stream.write_all(&payload).await.map_err(AppError::from)?;
    let mut response = String::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        BufReader::new(stream).read_line(&mut response),
    )
    .await
    .map_err(|_| {
        AppError::service_unavailable(
            "tun_helper_timeout",
            "macOS privileged helper did not respond",
        )
    })?
    .map_err(AppError::from)?;
    let response: MacosHelperResponse =
        serde_json::from_str(&response).map_err(AppError::internal)?;
    if response.ok {
        Ok(response)
    } else {
        Err(AppError::service_unavailable(
            "tun_helper_failed",
            response
                .error
                .unwrap_or_else(|| "macOS privileged helper rejected the request".into()),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct CoreManager {
    inner: Arc<CoreInner>,
}

#[derive(Debug)]
struct CoreInner {
    storage: Storage,
    operation: Mutex<()>,
    child: Mutex<Option<Child>>,
    #[cfg(any(target_os = "macos", test))]
    macos_tun: Mutex<Option<MacosTunSession>>,
    status: RwLock<CoreStatus>,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug)]
struct MacosTunSession {
    stop_path: std::path::PathBuf,
    _bridge: OwnedWriteHalf,
}

#[derive(Debug, Clone)]
struct CoreStatus {
    state: String,
    pid: Option<u32>,
    started_at: Option<String>,
    last_error: Option<String>,
    controller_addr: String,
    version: Option<String>,
}

impl Default for CoreStatus {
    fn default() -> Self {
        Self {
            state: "not_running".into(),
            pid: None,
            started_at: None,
            last_error: None,
            controller_addr: "127.0.0.1:9090".into(),
            version: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CoreStartConfig {
    pub controller_addr: String,
    pub controller_secret: String,
    pub controller_enabled: bool,
    pub mihomo_binary: std::path::PathBuf,
    pub runtime_yaml: std::path::PathBuf,
    pub runtime_dir: std::path::PathBuf,
    pub log_level: String,
    pub tun: bool,
}

impl CoreManager {
    pub fn new(_paths: AppPaths, storage: Storage) -> Self {
        Self {
            inner: Arc::new(CoreInner {
                storage,
                operation: Mutex::new(()),
                child: Mutex::new(None),
                #[cfg(any(target_os = "macos", test))]
                macos_tun: Mutex::new(None),
                status: RwLock::new(CoreStatus::default()),
            }),
        }
    }

    pub async fn snapshot(&self, controller_addr: String) -> CoreStatusResponse {
        let _ = self.sync_process_state(controller_addr.clone()).await;
        let status = self.inner.status.read().await.clone();
        CoreStatusResponse {
            state: status.state,
            pid: status.pid,
            started_at: status.started_at,
            last_error: status.last_error,
            controller_addr: if controller_addr.is_empty() {
                status.controller_addr
            } else {
                controller_addr
            },
            version: status.version,
        }
    }

    pub async fn is_running(&self) -> bool {
        let status = self.inner.status.read().await;
        matches!(status.state.as_str(), "running" | "starting")
    }

    pub async fn start(&self, config: CoreStartConfig) -> Result<CoreStatusResponse, AppError> {
        let _operation = self.inner.operation.lock().await;
        self.start_inner(config, false).await
    }

    async fn start_inner(
        &self,
        config: CoreStartConfig,
        config_validated: bool,
    ) -> Result<CoreStatusResponse, AppError> {
        info!(
            controller_addr = %config.controller_addr,
            mihomo_binary = %AppPaths::display(&config.mihomo_binary),
            runtime_yaml = %AppPaths::display(&config.runtime_yaml),
            runtime_dir = %AppPaths::display(&config.runtime_dir),
            "core start requested"
        );
        self.sync_process_state(config.controller_addr.clone())
            .await?;
        if self.is_running().await {
            info!("core is already running");
            return Ok(self.snapshot(config.controller_addr).await);
        }
        if let Err(error) = validate_candidate_files(&config) {
            self.mark_error(config.controller_addr.clone(), &error.message)
                .await;
            return Err(error);
        }

        self.set_status(CoreStatus {
            state: "starting".into(),
            pid: None,
            started_at: None,
            last_error: None,
            controller_addr: config.controller_addr.clone(),
            version: None,
        })
        .await;

        if !config_validated {
            info!("validating mihomo runtime config");
            if let Err(err) = self.validate_config(&config).await {
                self.mark_error(config.controller_addr.clone(), &err.message)
                    .await;
                return Err(err);
            }
            info!("mihomo runtime config validation passed");
        }

        let version = self.binary_version(&config.mihomo_binary).await;
        let pid = match self.spawn_core_process(&config).await {
            Ok(pid) => pid,
            Err(error) => {
                self.mark_error(config.controller_addr.clone(), &error.message)
                    .await;
                return Err(error);
            }
        };
        info!(pid = ?pid, version = ?version, "mihomo process spawned");

        if let Err(err) = self.wait_for_startup(&config).await {
            self.terminate_tracked_child().await;
            self.mark_error(config.controller_addr.clone(), &err.message)
                .await;
            return Err(err);
        }

        self.set_status(CoreStatus {
            state: "running".into(),
            pid,
            started_at: Some(now_iso()),
            last_error: None,
            controller_addr: config.controller_addr.clone(),
            version,
        })
        .await;
        Ok(self.snapshot(config.controller_addr).await)
    }

    async fn spawn_core_process(&self, config: &CoreStartConfig) -> Result<Option<u32>, AppError> {
        #[cfg(target_os = "macos")]
        if config.tun {
            return self.spawn_macos_tun_process(config).await;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = config.tun;

        let mut command = mihomo_command(&config.mihomo_binary);
        command
            .arg("-d")
            .arg(&config.runtime_dir)
            .arg("-f")
            .arg(&config.runtime_yaml)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn().map_err(AppError::from)?;
        let pid = child.id();
        if let Some(stdout) = child.stdout.take() {
            self.spawn_log_reader(
                stdout,
                "info".into(),
                "mihomo-stdout".into(),
                config.log_level.clone(),
            );
        }
        if let Some(stderr) = child.stderr.take() {
            self.spawn_log_reader(
                stderr,
                "warning".into(),
                "mihomo-stderr".into(),
                config.log_level.clone(),
            );
        }
        let mut guard = self.inner.child.lock().await;
        *guard = Some(child);
        Ok(pid)
    }

    #[cfg(any(target_os = "macos", test))]
    async fn spawn_macos_tun_process(
        &self,
        config: &CoreStartConfig,
    ) -> Result<Option<u32>, AppError> {
        if std::env::var_os("RWEB_CLASH_USE_PRIVILEGED_HELPER").is_some()
            && config
                .mihomo_binary
                .starts_with("/Library/Application Support/rweb-clash/")
        {
            let hash = content_hash(tokio::fs::read(&config.mihomo_binary).await?);
            let client_path = std::env::current_exe().map_err(macos_tun_setup_error)?;
            let response = macos_helper_request(serde_json::json!({
                "op": "start",
                "binary": config.mihomo_binary,
                "config": config.runtime_yaml,
                "state_dir": config.runtime_dir,
                "binary_sha256": hash,
                "client_path": client_path,
            }))
            .await?;
            let log_path = config.runtime_dir.join("mihomo.log");
            if let Ok(log) = tokio::fs::File::open(log_path).await {
                self.spawn_log_reader(
                    log,
                    "info".into(),
                    "mihomo-helper".into(),
                    config.log_level.clone(),
                );
            }
            return Ok(response.pid);
        }
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(macos_tun_setup_error)?;
        let port = listener.local_addr().map_err(macos_tun_setup_error)?.port();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let stop_path = std::env::temp_dir().join(format!(
            "rweb-clash-tun-stop-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let binary_hash = content_hash(tokio::fs::read(&config.mihomo_binary).await?);
        let runtime_hash = content_hash(tokio::fs::read(&config.runtime_yaml).await?);
        let geoip_path = config.runtime_dir.join("geoip.metadb");
        let geoip_hash = match tokio::fs::read(&geoip_path).await {
            Ok(bytes) => Some(content_hash(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(AppError::from(error)),
        };
        let shell = macos_tun_shell_command(
            config,
            &MacosTunShellParams {
                port,
                token: &token,
                stop_path: &stop_path,
                owner_pid: std::process::id(),
                binary_hash: &binary_hash,
                runtime_hash: &runtime_hash,
                geoip_hash: geoip_hash.as_deref(),
            },
        )?;
        let script = format!(
            "do shell script \"{}\" with administrator privileges",
            escape_applescript_string(&shell)
        );
        let mut command = Command::new("/usr/bin/osascript");
        command
            .args(["-e", &script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            AppError::service_unavailable(
                "tun_authorization_failed",
                format!("failed to request macOS administrator authorization for TUN: {error}"),
            )
        })?;

        let deadline = tokio::time::Instant::now() + MACOS_TUN_AUTHORIZATION_TIMEOUT;
        let (log_reader, bridge, pid) = loop {
            if let Some(status) = child.try_wait().map_err(AppError::from)? {
                let detail = macos_authorization_output(&mut child).await;
                return Err(AppError::service_unavailable(
                    "tun_authorization_failed",
                    if detail.is_empty() {
                        format!("macOS administrator authorization for TUN exited with {status}")
                    } else {
                        format!("macOS administrator authorization for TUN failed: {detail}")
                    },
                ));
            }
            if tokio::time::Instant::now() >= deadline {
                let _ = child.kill().await;
                return Err(AppError::service_unavailable(
                    "tun_authorization_failed",
                    "timed out waiting for macOS administrator authorization for TUN",
                ));
            }

            let (stream, _) =
                match tokio::time::timeout(Duration::from_millis(250), listener.accept()).await {
                    Ok(Ok(accepted)) => accepted,
                    Ok(Err(error)) => {
                        let _ = child.kill().await;
                        return Err(macos_tun_setup_error(error));
                    }
                    Err(_) => continue,
                };
            let (read_half, write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut received_token = String::new();
            let mut received_pid = String::new();
            let handshake = tokio::time::timeout(Duration::from_secs(2), async {
                reader.read_line(&mut received_token).await?;
                reader.read_line(&mut received_pid).await
            })
            .await;
            if !matches!(handshake, Ok(Ok(_))) || received_token.trim() != token {
                continue;
            }
            let Ok(pid) = received_pid.trim().parse::<u32>() else {
                continue;
            };
            if pid == 0 {
                continue;
            }
            break (reader, write_half, pid);
        };

        if let Some(stdout) = child.stdout.take() {
            self.spawn_log_reader(
                stdout,
                "info".into(),
                "macos-tun-authorization".into(),
                config.log_level.clone(),
            );
        }
        if let Some(stderr) = child.stderr.take() {
            self.spawn_log_reader(
                stderr,
                "warning".into(),
                "macos-tun-authorization".into(),
                config.log_level.clone(),
            );
        }
        self.spawn_log_reader(
            log_reader,
            "info".into(),
            "mihomo-stdout".into(),
            config.log_level.clone(),
        );
        {
            let mut guard = self.inner.child.lock().await;
            *guard = Some(child);
        }
        {
            let mut guard = self.inner.macos_tun.lock().await;
            *guard = Some(MacosTunSession {
                stop_path,
                _bridge: bridge,
            });
        }
        Ok(Some(pid))
    }

    #[cfg(any(target_os = "macos", test))]
    async fn signal_macos_tun_stop(&self) -> Option<std::path::PathBuf> {
        let session = {
            let mut guard = self.inner.macos_tun.lock().await;
            guard.take()
        }?;
        let stop_path = session.stop_path.clone();
        drop(session);
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stop_path)
            .await
        {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => warn!(
                path = %AppPaths::display(&stop_path),
                error = %error,
                "failed to create macOS TUN stop marker; closing the log bridge instead"
            ),
        }
        Some(stop_path)
    }

    async fn stop_child_process(&self, child: &mut Child) -> Result<(), AppError> {
        let pid = child.id();
        #[cfg(target_os = "macos")]
        let stop_path = self.signal_macos_tun_stop().await;
        #[cfg(target_os = "macos")]
        let graceful = stop_path.is_some();
        #[cfg(not(target_os = "macos"))]
        let graceful = false;

        let result = if graceful {
            info!(pid = ?pid, "requesting privileged mihomo process shutdown");
            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => {
                    info!(pid = ?pid, %status, "privileged mihomo process stopped");
                    Ok(())
                }
                Ok(Err(error)) => {
                    warn!(pid = ?pid, error = %error, "failed waiting for privileged mihomo process");
                    Ok(())
                }
                Err(_) => {
                    warn!(pid = ?pid, "privileged mihomo shutdown timed out; terminating authorization process");
                    if child.try_wait().map_err(AppError::from)?.is_none() {
                        child.kill().await.map_err(AppError::from)?;
                    }
                    match child.wait().await {
                        Ok(status) => {
                            info!(pid = ?pid, %status, "authorization process terminated")
                        }
                        Err(error) => {
                            warn!(pid = ?pid, error = %error, "failed waiting for authorization process")
                        }
                    }
                    Ok(())
                }
            }
        } else {
            if child.try_wait().map_err(AppError::from)?.is_none() {
                info!(pid = ?pid, "killing mihomo process");
                child.kill().await.map_err(AppError::from)?;
            }
            match child.wait().await {
                Ok(status) => info!(pid = ?pid, %status, "mihomo process stopped"),
                Err(error) => {
                    warn!(pid = ?pid, error = %error, "failed waiting for mihomo process")
                }
            }
            Ok(())
        };

        #[cfg(target_os = "macos")]
        if let Some(stop_path) = stop_path {
            if let Err(error) = tokio::fs::remove_file(&stop_path).await {
                if error.kind() != std::io::ErrorKind::NotFound {
                    warn!(
                        path = %AppPaths::display(&stop_path),
                        error = %error,
                        "failed to remove macOS TUN stop marker"
                    );
                }
            }
        }
        result
    }

    pub async fn stop(&self, controller_addr: String) -> Result<CoreStatusResponse, AppError> {
        let _operation = self.inner.operation.lock().await;
        self.stop_inner(controller_addr).await
    }

    async fn stop_inner(&self, controller_addr: String) -> Result<CoreStatusResponse, AppError> {
        info!(controller_addr = %controller_addr, "core stop requested");
        self.set_status(CoreStatus {
            state: "stopping".into(),
            pid: None,
            started_at: None,
            last_error: None,
            controller_addr: controller_addr.clone(),
            version: None,
        })
        .await;

        let child = {
            let mut guard = self.inner.child.lock().await;
            guard.take()
        };
        if let Some(mut child) = child {
            self.stop_child_process(&mut child).await?;
        } else {
            info!("core stop requested, no child process was tracked");
            #[cfg(target_os = "macos")]
            {
                if std::env::var_os("RWEB_CLASH_USE_PRIVILEGED_HELPER").is_some() {
                    macos_helper_request(serde_json::json!({"op":"stop"})).await?;
                }
                self.clear_macos_tun_session().await;
            }
        }

        self.set_status(CoreStatus {
            controller_addr: controller_addr.clone(),
            ..CoreStatus::default()
        })
        .await;
        Ok(self.snapshot(controller_addr).await)
    }

    pub async fn restart(&self, config: CoreStartConfig) -> Result<CoreStatusResponse, AppError> {
        let _operation = self.inner.operation.lock().await;
        info!(controller_addr = %config.controller_addr, "core restart requested");
        validate_candidate_files(&config)?;
        info!("validating mihomo runtime config before stopping the running core");
        self.validate_config(&config).await?;
        info!("mihomo runtime config validation passed");
        let _ = self.stop_inner(config.controller_addr.clone()).await?;
        self.start_inner(config, true).await
    }

    async fn validate_config(&self, config: &CoreStartConfig) -> Result<(), AppError> {
        validate_mihomo_config(
            &config.mihomo_binary,
            &config.runtime_dir,
            &config.runtime_yaml,
        )
        .await
    }

    async fn wait_for_startup(&self, config: &CoreStartConfig) -> Result<(), AppError> {
        if !config.controller_enabled {
            tokio::time::sleep(Duration::from_millis(500)).await;
            if let Some(status) = self.tracked_child_exit_status().await? {
                return Err(AppError::service_unavailable(
                    "core_start_failed",
                    format!("mihomo exited during startup with status {status}"),
                ));
            }
            return Ok(());
        }

        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .map_err(AppError::internal)?;
        let url = controller_url(&config.controller_addr, "/version");
        let deadline = tokio::time::Instant::now() + CORE_STARTUP_TIMEOUT;
        let last_error = loop {
            if let Some(status) = self.tracked_child_exit_status().await? {
                return Err(AppError::service_unavailable(
                    "core_start_failed",
                    format!("mihomo exited during startup with status {status}"),
                ));
            }

            let mut request = client.get(&url);
            if !config.controller_secret.trim().is_empty() {
                request = request.bearer_auth(config.controller_secret.trim());
            }
            let attempt_error =
                match tokio::time::timeout(Duration::from_millis(300), request.send()).await {
                    Ok(Ok(response)) if response.status().is_success() => return Ok(()),
                    Ok(Ok(response)) => format!("controller returned {}", response.status()),
                    Ok(Err(err)) => err.to_string(),
                    Err(_) => "controller health check timed out".into(),
                };
            if tokio::time::Instant::now() >= deadline {
                break attempt_error;
            }
            tokio::time::sleep(CORE_STARTUP_POLL_INTERVAL).await;
        };

        // The privileged macOS wrapper can exit just after the last request fails.
        // Give process state propagation one final chance so callers see the exit
        // instead of the less useful controller-unreachable message.
        tokio::time::sleep(CORE_STARTUP_POLL_INTERVAL).await;
        if let Some(status) = self.tracked_child_exit_status().await? {
            return Err(AppError::service_unavailable(
                "core_start_failed",
                format!("mihomo exited during startup with status {status}"),
            ));
        }

        Err(AppError::service_unavailable(
            "controller_unreachable",
            format!(
                "mihomo started but external-controller was not reachable at {}: {}",
                config.controller_addr, last_error
            ),
        ))
    }

    async fn tracked_child_exit_status(
        &self,
    ) -> Result<Option<std::process::ExitStatus>, AppError> {
        let status = {
            let mut guard = self.inner.child.lock().await;
            if let Some(mut child) = guard.take() {
                match child.try_wait().map_err(AppError::from)? {
                    Some(status) => Some(status),
                    None => {
                        *guard = Some(child);
                        None
                    }
                }
            } else {
                None
            }
        };
        #[cfg(target_os = "macos")]
        if status.is_some() {
            self.clear_macos_tun_session().await;
        }
        Ok(status)
    }

    #[cfg(any(target_os = "macos", test))]
    async fn clear_macos_tun_session(&self) {
        let session = {
            let mut guard = self.inner.macos_tun.lock().await;
            guard.take()
        };
        if let Some(session) = session {
            let stop_path = session.stop_path.clone();
            drop(session);
            if let Err(error) = tokio::fs::remove_file(&stop_path).await {
                if error.kind() != std::io::ErrorKind::NotFound {
                    warn!(
                        path = %AppPaths::display(&stop_path),
                        error = %error,
                        "failed to remove stale macOS TUN stop marker"
                    );
                }
            }
        }
    }

    async fn terminate_tracked_child(&self) {
        let child = {
            let mut guard = self.inner.child.lock().await;
            guard.take()
        };
        if let Some(mut child) = child {
            if let Err(error) = self.stop_child_process(&mut child).await {
                warn!(error = %error, "failed terminating mihomo after failed startup");
            }
        } else {
            #[cfg(target_os = "macos")]
            self.clear_macos_tun_session().await;
        }
    }

    async fn binary_version(&self, binary: &std::path::Path) -> Option<String> {
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            mihomo_command(binary).arg("-v").kill_on_drop(true).output(),
        )
        .await
        .ok()?
        .ok()?;
        if !output.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        text.lines()
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    async fn sync_process_state(&self, controller_addr: String) -> Result<(), AppError> {
        let exited = {
            let mut guard = self.inner.child.lock().await;
            if let Some(mut child) = guard.take() {
                match child.try_wait().map_err(AppError::from)? {
                    Some(status) => Some(status),
                    None => {
                        *guard = Some(child);
                        None
                    }
                }
            } else {
                None
            }
        };
        if let Some(status) = exited {
            #[cfg(target_os = "macos")]
            self.clear_macos_tun_session().await;
            let message = format!("mihomo exited unexpectedly with status {status}");
            warn!(%status, "tracked mihomo process exited unexpectedly");
            self.mark_error(controller_addr, &message).await;
        }
        Ok(())
    }

    async fn mark_error(&self, controller_addr: String, message: &str) {
        warn!("{message}");
        let _ = self
            .inner
            .storage
            .append_log("error", message, parse_host_from_log(message).as_deref())
            .await;
        self.set_status(CoreStatus {
            state: "error".into(),
            pid: None,
            started_at: None,
            last_error: Some(message.to_string()),
            controller_addr,
            version: None,
        })
        .await;
    }

    async fn set_status(&self, status: CoreStatus) {
        info!(
            state = %status.state,
            pid = ?status.pid,
            controller_addr = %status.controller_addr,
            "core status updated"
        );
        let mut guard = self.inner.status.write().await;
        *guard = status;
    }

    fn spawn_log_reader<T>(
        &self,
        reader: T,
        fallback_level: String,
        source: String,
        minimum_level: String,
    ) where
        T: tokio::io::AsyncRead + Send + Unpin + 'static,
    {
        let storage = self.inner.storage.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let level = detected_log_level(&line, &fallback_level);
                if !log_level_is_enabled(&level, &minimum_level) {
                    continue;
                }
                let payload = format!("{source}: {line}");
                match level.as_str() {
                    "error" => error!("{payload}"),
                    "warning" => warn!("{payload}"),
                    _ => info!("{payload}"),
                }
                let parsed = parse_host_from_log(&payload);
                let _ = storage
                    .append_log(&level, &payload, parsed.as_deref())
                    .await;
            }
        });
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_tun_setup_error(error: impl std::fmt::Display) -> AppError {
    AppError::service_unavailable(
        "tun_authorization_failed",
        format!("failed to prepare macOS administrator authorization for TUN: {error}"),
    )
}

#[cfg(any(target_os = "macos", test))]
async fn macos_authorization_output(child: &mut Child) -> String {
    let mut parts = Vec::new();
    if let Some(mut stderr) = child.stderr.take() {
        let mut text = String::new();
        if stderr.read_to_string(&mut text).await.is_ok() && !text.trim().is_empty() {
            parts.push(text.trim().to_string());
        }
    }
    if let Some(mut stdout) = child.stdout.take() {
        let mut text = String::new();
        if stdout.read_to_string(&mut text).await.is_ok() && !text.trim().is_empty() {
            parts.push(text.trim().to_string());
        }
    }
    parts.join("; ")
}

#[cfg(any(target_os = "macos", test))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(any(target_os = "macos", test))]
fn escape_applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(any(target_os = "macos", test))]
struct MacosTunShellParams<'a> {
    port: u16,
    token: &'a str,
    stop_path: &'a std::path::Path,
    owner_pid: u32,
    binary_hash: &'a str,
    runtime_hash: &'a str,
    geoip_hash: Option<&'a str>,
}

#[cfg(any(target_os = "macos", test))]
fn macos_tun_shell_command(
    config: &CoreStartConfig,
    params: &MacosTunShellParams<'_>,
) -> Result<String, AppError> {
    fn quoted_path(path: &std::path::Path, label: &str) -> Result<String, AppError> {
        if !path.is_absolute() {
            return Err(AppError::bad_request(
                "tun_path_invalid",
                format!(
                    "macOS TUN {label} path must be absolute: {}",
                    path.display()
                ),
            ));
        }
        let value = path.to_str().ok_or_else(|| {
            AppError::bad_request(
                "tun_path_invalid",
                format!(
                    "macOS TUN {label} path is not valid UTF-8: {}",
                    path.display()
                ),
            )
        })?;
        Ok(shell_single_quote(value))
    }

    let binary = quoted_path(&config.mihomo_binary, "binary")?;
    let runtime_yaml = quoted_path(&config.runtime_yaml, "runtime config")?;
    let safe_paths = quoted_path(&config.runtime_dir, "runtime directory")?;
    let stop_path = quoted_path(params.stop_path, "stop marker")?;
    let token = shell_single_quote(params.token);
    let binary_hash = shell_single_quote(params.binary_hash);
    let runtime_hash = shell_single_quote(params.runtime_hash);
    let geoip_stage = if let Some(geoip_hash) = params.geoip_hash {
        let geoip_database =
            quoted_path(&config.runtime_dir.join("geoip.metadb"), "GeoIP database")?;
        let geoip_hash = shell_single_quote(geoip_hash);
        format!(
            concat!(
                "/bin/cp {geoip_database} \"$runtime_home/geoip.metadb\" || exit 1; ",
                "[ \"$(/usr/bin/shasum -a 256 \"$runtime_home/geoip.metadb\" | /usr/bin/awk '{{print $1}}')\" = {geoip_hash} ] || exit 1; "
            ),
            geoip_database = geoip_database,
            geoip_hash = geoip_hash,
        )
    } else {
        String::new()
    };
    Ok(format!(
        concat!(
            "core_pid=''; bridge_pid=''; requested_stop=0; ",
            "tmpdir=$(/usr/bin/mktemp -d /private/tmp/rweb-clash-tun.XXXXXX) || exit 1; ",
            "cleanup() {{ ",
            "if [ -n \"$core_pid\" ] && /bin/kill -0 \"$core_pid\" 2>/dev/null; then /bin/kill -TERM \"$core_pid\" 2>/dev/null || true; fi; ",
            "if [ -n \"$bridge_pid\" ]; then /bin/kill \"$bridge_pid\" 2>/dev/null || true; fi; ",
            "/bin/rm -rf \"$tmpdir\"; ",
            "}}; ",
            "trap cleanup EXIT; trap 'requested_stop=1; exit 0' HUP INT TERM; ",
            "log_file=\"$tmpdir/mihomo.log\"; /usr/bin/touch \"$log_file\" || exit 1; ",
            "runtime_home=\"$tmpdir/home\"; /bin/mkdir -m 700 \"$runtime_home\" || exit 1; ",
            "/bin/cp {binary} \"$runtime_home/mihomo\" || exit 1; ",
            "[ \"$(/usr/bin/shasum -a 256 \"$runtime_home/mihomo\" | /usr/bin/awk '{{print $1}}')\" = {binary_hash} ] || exit 1; ",
            "/bin/chmod 700 \"$runtime_home/mihomo\" || exit 1; ",
            "/bin/cp {runtime_yaml} \"$runtime_home/runtime.yaml\" || exit 1; ",
            "[ \"$(/usr/bin/shasum -a 256 \"$runtime_home/runtime.yaml\" | /usr/bin/awk '{{print $1}}')\" = {runtime_hash} ] || exit 1; ",
            "{geoip_stage}",
            "SAFE_PATHS={safe_paths} \"$runtime_home/mihomo\" -d \"$runtime_home\" -f \"$runtime_home/runtime.yaml\" >\"$log_file\" 2>&1 & core_pid=$!; ",
            "( /usr/bin/printf '%s\\n%s\\n' {token} \"$core_pid\"; /usr/bin/tail -n +1 -f \"$log_file\"; ) | /usr/bin/nc 127.0.0.1 {port} & bridge_pid=$!; ",
            "while /bin/kill -0 \"$core_pid\" 2>/dev/null; do ",
            "if ! /bin/kill -0 {owner_pid} 2>/dev/null || ! /bin/kill -0 \"$bridge_pid\" 2>/dev/null || [ -e {stop_path} ]; then ",
            "requested_stop=1; /bin/kill -TERM \"$core_pid\" 2>/dev/null || true; break; fi; ",
            "/bin/sleep 0.2; done; ",
            "core_status=0; wait \"$core_pid\" || core_status=$?; ",
            "/bin/kill \"$bridge_pid\" 2>/dev/null || true; wait \"$bridge_pid\" 2>/dev/null || true; ",
            "bridge_pid=''; core_pid=''; ",
            "if [ \"$requested_stop\" -eq 1 ]; then exit 0; fi; exit \"$core_status\""
        ),
        binary = binary,
        runtime_yaml = runtime_yaml,
        safe_paths = safe_paths,
        binary_hash = binary_hash,
        runtime_hash = runtime_hash,
        geoip_stage = geoip_stage,
        token = token,
        port = params.port,
        owner_pid = params.owner_pid,
        stop_path = stop_path,
    ))
}

fn detected_log_level(line: &str, fallback: &str) -> String {
    let uppercase = line.to_ascii_uppercase();
    if uppercase.contains("[ERROR]") || uppercase.contains(" LEVEL=ERROR") {
        "error".into()
    } else if uppercase.contains("[WARN]")
        || uppercase.contains("[WARNING]")
        || uppercase.contains(" LEVEL=WARN")
    {
        "warning".into()
    } else if uppercase.contains("[DEBUG]") || uppercase.contains(" LEVEL=DEBUG") {
        "debug".into()
    } else {
        fallback.to_string()
    }
}

fn log_level_is_enabled(level: &str, minimum: &str) -> bool {
    fn rank(level: &str) -> u8 {
        match level {
            "debug" => 1,
            "info" => 2,
            "warning" | "warn" => 3,
            "error" => 4,
            "silent" => 5,
            _ => 2,
        }
    }
    minimum != "silent" && rank(level) >= rank(minimum)
}

pub(crate) async fn validate_mihomo_config(
    mihomo_binary: &std::path::Path,
    runtime_dir: &std::path::Path,
    runtime_yaml: &std::path::Path,
) -> Result<(), AppError> {
    if !mihomo_binary.is_file() {
        return Err(AppError::bad_request(
            "core_binary_missing",
            format!(
                "mihomo binary not found at {}",
                AppPaths::display(mihomo_binary)
            ),
        ));
    }
    if !runtime_yaml.is_file() {
        return Err(AppError::bad_request(
            "runtime_config_missing",
            format!(
                "runtime config not found at {}",
                AppPaths::display(runtime_yaml)
            ),
        ));
    }
    let validation_timeout = mihomo_validation_timeout();
    let output = tokio::time::timeout(
        validation_timeout,
        mihomo_command(mihomo_binary)
            .arg("-t")
            .arg("-d")
            .arg(runtime_dir)
            .arg("-f")
            .arg(runtime_yaml)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        AppError::new(
            StatusCode::BAD_REQUEST,
            "core_config_validation_timeout",
            "mihomo config validation timed out",
        )
    })?
    .map_err(AppError::from)?;

    if output.status.success() {
        info!(status = %output.status, "mihomo config test succeeded");
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if stderr.is_empty() { stdout } else { stderr };
    warn!(status = %output.status, message = %message, "mihomo config test failed");
    Err(AppError::bad_request(
        "core_config_invalid",
        if message.is_empty() {
            "mihomo rejected generated runtime config".into()
        } else {
            message
        },
    ))
}

fn mihomo_validation_timeout() -> Duration {
    let configured = std::env::var(MIHOMO_VALIDATION_TIMEOUT_ENV).ok();
    Duration::from_secs(validation_timeout_seconds(configured.as_deref()))
}

fn validation_timeout_seconds(configured: Option<&str>) -> u64 {
    let Some(raw) = configured else {
        return DEFAULT_MIHOMO_VALIDATION_TIMEOUT_SECS;
    };
    match raw.trim().parse::<u64>() {
        Ok(seconds) if (1..=MAX_MIHOMO_VALIDATION_TIMEOUT_SECS).contains(&seconds) => seconds,
        _ => {
            warn!(
                environment = MIHOMO_VALIDATION_TIMEOUT_ENV,
                default_seconds = DEFAULT_MIHOMO_VALIDATION_TIMEOUT_SECS,
                max_seconds = MAX_MIHOMO_VALIDATION_TIMEOUT_SECS,
                "ignoring invalid Mihomo validation timeout"
            );
            DEFAULT_MIHOMO_VALIDATION_TIMEOUT_SECS
        }
    }
}

fn validate_candidate_files(config: &CoreStartConfig) -> Result<(), AppError> {
    if !config.mihomo_binary.exists() {
        return Err(AppError::bad_request(
            "core_binary_missing",
            format!(
                "mihomo binary not found at {}",
                AppPaths::display(&config.mihomo_binary)
            ),
        ));
    }
    if !config.runtime_yaml.exists() {
        return Err(AppError::bad_request(
            "runtime_config_missing",
            format!(
                "runtime config not found at {}",
                AppPaths::display(&config.runtime_yaml)
            ),
        ));
    }
    Ok(())
}

fn controller_url(addr: &str, path: &str) -> String {
    let base = if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.trim_end_matches('/').to_string()
    } else {
        format!("http://{}", addr.trim_end_matches('/'))
    };
    format!("{base}{path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_timeout_defaults_and_rejects_invalid_extremes() {
        assert_eq!(validation_timeout_seconds(None), 120);
        assert_eq!(validation_timeout_seconds(Some(" 45 ")), 45);
        assert_eq!(validation_timeout_seconds(Some("3600")), 3600);
        assert_eq!(validation_timeout_seconds(Some("")), 120);
        assert_eq!(validation_timeout_seconds(Some("0")), 120);
        assert_eq!(validation_timeout_seconds(Some("-1")), 120);
        assert_eq!(validation_timeout_seconds(Some("3601")), 120);
        assert_eq!(
            validation_timeout_seconds(Some("18446744073709551615")),
            120
        );
        assert_eq!(validation_timeout_seconds(Some("not-a-number")), 120);
    }

    #[test]
    fn macos_tun_shell_values_are_quoted_for_shell_and_applescript() {
        let _ = CoreManager::spawn_macos_tun_process;
        let _ = CoreManager::signal_macos_tun_stop;
        let _ = CoreManager::clear_macos_tun_session;

        assert_eq!(shell_single_quote("a'b"), r#"'a'\''b'"#);
        assert_eq!(escape_applescript_string(r#"a\b\"c"#), r#"a\\b\\\"c"#);

        let root = std::env::current_dir()
            .expect("current directory")
            .join("macos tun 'fixture'");
        let config = CoreStartConfig {
            controller_addr: "127.0.0.1:9090".into(),
            controller_secret: String::new(),
            controller_enabled: true,
            mihomo_binary: root.join("mihomo"),
            runtime_yaml: root.join("runtime.yaml"),
            runtime_dir: root.join("profiles"),
            log_level: "info".into(),
            tun: true,
        };
        let stop_path = root.join("stop");
        let command = macos_tun_shell_command(
            &config,
            &MacosTunShellParams {
                port: 32123,
                token: "abc'def",
                stop_path: &stop_path,
                owner_pid: 42,
                binary_hash: &"a".repeat(64),
                runtime_hash: &"b".repeat(64),
                geoip_hash: Some(&"c".repeat(64)),
            },
        )
        .expect("build privileged shell command");

        assert!(command.contains(&shell_single_quote(
            config.mihomo_binary.to_str().expect("binary path")
        )));
        assert!(command.contains(&shell_single_quote(
            config.runtime_yaml.to_str().expect("runtime path")
        )));
        assert!(command.contains(&shell_single_quote(
            config
                .runtime_dir
                .join("geoip.metadb")
                .to_str()
                .expect("GeoIP path")
        )));
        assert!(command.contains(r#""$runtime_home/mihomo" -d "$runtime_home""#));
        assert!(command.contains(&format!(
            "SAFE_PATHS={} ",
            shell_single_quote(config.runtime_dir.to_str().expect("runtime directory"))
        )));
        assert!(command.contains("/usr/bin/shasum -a 256"));
        assert!(command.contains(r#"/usr/bin/tail -n +1 -f "$log_file""#));
        assert!(!command.contains("mkfifo"));
        assert!(command.contains("/usr/bin/nc 127.0.0.1 32123"));
        assert!(command.contains("/bin/kill -0 42"));
        assert!(command.contains(r#"! /bin/kill -0 "$bridge_pid""#));
        assert!(command.contains(r#"[ -e "#));
        assert!(command.contains(r#"/bin/kill -TERM "$core_pid""#));
        assert!(command.contains("trap cleanup EXIT"));
        assert!(command.contains(&shell_single_quote("abc'def")));
        assert!(command.contains(&shell_single_quote(stop_path.to_str().expect("stop path"))));

        #[cfg(unix)]
        assert!(std::process::Command::new("/bin/sh")
            .args(["-n", "-c", &command])
            .status()
            .expect("parse privileged shell command")
            .success());
    }

    #[tokio::test]
    async fn invalid_restart_candidate_preserves_running_status() {
        let temp = TestDir::new("core-restart-validation");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app directories");
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        let manager = CoreManager::new(paths.clone(), storage);
        manager
            .set_status(CoreStatus {
                state: "running".into(),
                pid: Some(42),
                started_at: Some(now_iso()),
                last_error: None,
                controller_addr: "127.0.0.1:9090".into(),
                version: Some("test".into()),
            })
            .await;

        let error = manager
            .restart(CoreStartConfig {
                controller_addr: "127.0.0.1:9090".into(),
                controller_secret: String::new(),
                controller_enabled: false,
                mihomo_binary: temp.path().join("missing-mihomo"),
                runtime_yaml: temp.path().join("missing-runtime.yaml"),
                runtime_dir: paths.profiles_dir,
                log_level: "info".into(),
                tun: false,
            })
            .await
            .expect_err("invalid candidate must be rejected");

        assert_eq!(error.code, "core_binary_missing");
        let status = manager.inner.status.read().await.clone();
        assert_eq!(status.state, "running");
        assert_eq!(status.pid, Some(42));
        assert_eq!(status.version.as_deref(), Some("test"));
    }

    #[tokio::test]
    async fn spawn_failure_sets_error_status() {
        let temp = TestDir::new("core-spawn-failure");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app directories");
        tokio::fs::write(&paths.runtime_yaml, "mixed-port: 7890\n")
            .await
            .expect("write runtime");
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        let manager = CoreManager::new(paths.clone(), storage);

        let error = manager
            .start_inner(
                CoreStartConfig {
                    controller_addr: "127.0.0.1:9090".into(),
                    controller_secret: String::new(),
                    controller_enabled: false,
                    // An existing directory passes the file-presence guard but cannot be spawned.
                    mihomo_binary: temp.path().to_path_buf(),
                    runtime_yaml: paths.runtime_yaml,
                    runtime_dir: paths.profiles_dir,
                    log_level: "info".into(),
                    tun: false,
                },
                true,
            )
            .await
            .expect_err("spawning a directory must fail");

        assert_eq!(error.status, StatusCode::INTERNAL_SERVER_ERROR);
        let status = manager.inner.status.read().await.clone();
        assert_eq!(status.state, "error");
        assert!(status.last_error.is_some());
    }

    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("rweb-clash-{name}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create test directory");
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
