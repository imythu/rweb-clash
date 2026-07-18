use crate::error::AppError;
use crate::paths::AppPaths;
use crate::storage::Storage;
use crate::types::CoreStatusResponse;
use crate::util::{now_iso, parse_host_from_log};
use axum::http::StatusCode;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info, warn};

const DEFAULT_MIHOMO_VALIDATION_TIMEOUT_SECS: u64 = 120;
const MAX_MIHOMO_VALIDATION_TIMEOUT_SECS: u64 = 3_600;
const MIHOMO_VALIDATION_TIMEOUT_ENV: &str = "RWEB_CLASH_MIHOMO_VALIDATION_TIMEOUT_SECS";

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

#[derive(Debug, Clone)]
pub struct CoreManager {
    inner: Arc<CoreInner>,
}

#[derive(Debug)]
struct CoreInner {
    storage: Storage,
    operation: Mutex<()>,
    child: Mutex<Option<Child>>,
    status: RwLock<CoreStatus>,
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
}

impl CoreManager {
    pub fn new(_paths: AppPaths, storage: Storage) -> Self {
        Self {
            inner: Arc::new(CoreInner {
                storage,
                operation: Mutex::new(()),
                child: Mutex::new(None),
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
        let mut command = mihomo_command(&config.mihomo_binary);
        command
            .arg("-d")
            .arg(&config.runtime_dir)
            .arg("-f")
            .arg(&config.runtime_yaml)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let error = AppError::from(error);
                self.mark_error(config.controller_addr.clone(), &error.message)
                    .await;
                return Err(error);
            }
        };
        let pid = child.id();
        info!(pid = ?pid, version = ?version, "mihomo process spawned");
        if let Some(stdout) = child.stdout.take() {
            self.spawn_log_reader(stdout, "info".into(), "mihomo-stdout".into());
        }
        if let Some(stderr) = child.stderr.take() {
            self.spawn_log_reader(stderr, "warning".into(), "mihomo-stderr".into());
        }
        {
            let mut guard = self.inner.child.lock().await;
            *guard = Some(child);
        }

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
            let pid = child.id();
            if child.try_wait().map_err(AppError::from)?.is_none() {
                info!(pid = ?pid, "killing mihomo process");
                child.kill().await.map_err(AppError::from)?;
            }
            match child.wait().await {
                Ok(status) => info!(pid = ?pid, %status, "mihomo process stopped"),
                Err(err) => warn!(pid = ?pid, error = %err, "failed waiting for mihomo process"),
            }
        } else {
            info!("core stop requested, no child process was tracked");
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
        let mut last_error = None;
        for _ in 0..30 {
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
            match tokio::time::timeout(Duration::from_millis(300), request.send()).await {
                Ok(Ok(response)) if response.status().is_success() => return Ok(()),
                Ok(Ok(response)) => {
                    last_error = Some(format!("controller returned {}", response.status()))
                }
                Ok(Err(err)) => last_error = Some(err.to_string()),
                Err(_) => last_error = Some("controller health check timed out".into()),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Err(AppError::service_unavailable(
            "controller_unreachable",
            format!(
                "mihomo started but external-controller was not reachable at {}: {}",
                config.controller_addr,
                last_error.unwrap_or_else(|| "unknown error".into())
            ),
        ))
    }

    async fn tracked_child_exit_status(
        &self,
    ) -> Result<Option<std::process::ExitStatus>, AppError> {
        let mut guard = self.inner.child.lock().await;
        if let Some(mut child) = guard.take() {
            match child.try_wait().map_err(AppError::from)? {
                Some(status) => Ok(Some(status)),
                None => {
                    *guard = Some(child);
                    Ok(None)
                }
            }
        } else {
            Ok(None)
        }
    }

    async fn terminate_tracked_child(&self) {
        let child = {
            let mut guard = self.inner.child.lock().await;
            guard.take()
        };
        if let Some(mut child) = child {
            let pid = child.id();
            if matches!(child.try_wait(), Ok(None)) {
                let _ = child.kill().await;
            }
            match child.wait().await {
                Ok(status) => {
                    info!(pid = ?pid, %status, "mihomo process terminated after failed startup")
                }
                Err(err) => {
                    warn!(pid = ?pid, error = %err, "failed waiting for mihomo after failed startup")
                }
            }
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

    fn spawn_log_reader<T>(&self, reader: T, level: String, source: String)
    where
        T: tokio::io::AsyncRead + Send + Unpin + 'static,
    {
        let storage = self.inner.storage.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
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
