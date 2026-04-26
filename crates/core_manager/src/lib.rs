use platform_linux::AppPaths;
use shared_types::{CoreRunState, CoreStatusResponse, LogEntry, ServerEvent};
use std::process::Stdio;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::{timeout, Duration};
use tracing::{error, info, warn};

const CONFIG_TEST_TIMEOUT: Duration = Duration::from_secs(60);
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct CoreStartConfig {
    pub active_profile_id: Option<String>,
    pub controller_addr: String,
    pub mihomo_binary: std::path::PathBuf,
    pub runtime_config: std::path::PathBuf,
    pub runtime_dir: std::path::PathBuf,
}

#[derive(Debug, Clone)]
pub struct CoreManager {
    paths: AppPaths,
    child: Arc<Mutex<Option<Child>>>,
    status: Arc<RwLock<RuntimeStatus>>,
    events: broadcast::Sender<ServerEvent>,
}

#[derive(Debug, Clone)]
struct RuntimeStatus {
    state: CoreRunState,
    pid: Option<u32>,
    active_profile_id: Option<String>,
    started_at: Option<String>,
    last_error: Option<String>,
    controller_addr: String,
    version: Option<String>,
}

impl Default for RuntimeStatus {
    fn default() -> Self {
        Self {
            state: CoreRunState::NotRunning,
            pid: None,
            active_profile_id: None,
            started_at: None,
            last_error: None,
            controller_addr: String::new(),
            version: None,
        }
    }
}

impl CoreManager {
    pub fn new(paths: AppPaths, events: broadcast::Sender<ServerEvent>) -> Self {
        Self {
            paths,
            child: Arc::new(Mutex::new(None)),
            status: Arc::new(RwLock::new(RuntimeStatus::default())),
            events,
        }
    }

    pub async fn snapshot(
        &self,
        active_profile_id: Option<String>,
        controller_addr: String,
    ) -> Result<CoreStatusResponse, CoreManagerError> {
        self.sync_process_state(active_profile_id.clone(), controller_addr.clone())
            .await?;

        let status = self.status.read().await.clone();
        Ok(CoreStatusResponse {
            state: status.state,
            pid: status.pid,
            active_profile_id: active_profile_id.or(status.active_profile_id),
            started_at: status.started_at,
            last_error: status.last_error,
            controller_addr: if controller_addr.is_empty() {
                status.controller_addr
            } else {
                controller_addr
            },
            version: status.version,
        })
    }

    pub async fn is_running(&self) -> Result<bool, CoreManagerError> {
        let current = self.status.read().await.state.clone();
        Ok(matches!(
            current,
            CoreRunState::Running | CoreRunState::Starting
        ))
    }

    pub async fn start(
        &self,
        config: CoreStartConfig,
    ) -> Result<CoreStatusResponse, CoreManagerError> {
        self.sync_process_state(
            config.active_profile_id.clone(),
            config.controller_addr.clone(),
        )
        .await?;

        if self.is_running().await? {
            return self
                .snapshot(config.active_profile_id, config.controller_addr)
                .await;
        }

        let mut status = self.current_runtime_status().await;
        status.state = CoreRunState::Starting;
        self.update_state(status).await;

        if let Err(err) = self.validate_config(&config).await {
            self.mark_start_error(
                config.active_profile_id.clone(),
                config.controller_addr.clone(),
                err.to_string(),
            )
            .await;
            return Err(err);
        }
        let version = self.binary_version(&config.mihomo_binary).await;

        info!(
            binary = %config.mihomo_binary.display(),
            runtime_dir = %config.runtime_dir.display(),
            runtime_config = %config.runtime_config.display(),
            "spawning mihomo process"
        );
        let mut command = Command::new(&config.mihomo_binary);
        command
            .arg("-d")
            .arg(&config.runtime_dir)
            .arg("-f")
            .arg(&config.runtime_config)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => {
                let err = CoreManagerError::Io(err);
                self.mark_start_error(
                    config.active_profile_id.clone(),
                    config.controller_addr.clone(),
                    err.to_string(),
                )
                .await;
                return Err(err);
            }
        };
        let pid = child.id();

        if let Some(stdout) = child.stdout.take() {
            self.spawn_log_reader(stdout, "mihomo-stdout".into(), "info".into());
        }
        if let Some(stderr) = child.stderr.take() {
            self.spawn_log_reader(stderr, "mihomo-stderr".into(), "warn".into());
        }

        {
            let mut guard = self.child.lock().await;
            *guard = Some(child);
        }

        self.update_state(RuntimeStatus {
            state: CoreRunState::Running,
            pid,
            active_profile_id: config.active_profile_id.clone(),
            started_at: Some(now_iso()),
            last_error: None,
            controller_addr: config.controller_addr.clone(),
            version,
        })
        .await;

        self.snapshot(config.active_profile_id, config.controller_addr)
            .await
    }

    pub async fn stop(
        &self,
        active_profile_id: Option<String>,
        controller_addr: String,
    ) -> Result<CoreStatusResponse, CoreManagerError> {
        let mut status = self.current_runtime_status().await;
        status.state = CoreRunState::Stopping;
        self.update_state(status).await;

        let child = {
            let mut guard = self.child.lock().await;
            guard.take()
        };

        if let Some(mut child) = child {
            if child.try_wait().map_err(CoreManagerError::Io)?.is_none() {
                child.kill().await.map_err(CoreManagerError::Io)?;
            }
            let _ = child.wait().await.map_err(CoreManagerError::Io)?;
        }

        self.update_state(RuntimeStatus::default()).await;
        self.snapshot(active_profile_id, controller_addr).await
    }

    pub async fn restart(
        &self,
        config: CoreStartConfig,
    ) -> Result<CoreStatusResponse, CoreManagerError> {
        let _ = self
            .stop(
                config.active_profile_id.clone(),
                config.controller_addr.clone(),
            )
            .await?;
        self.start(config).await
    }

    async fn validate_config(&self, config: &CoreStartConfig) -> Result<(), CoreManagerError> {
        info!(
            binary = %config.mihomo_binary.display(),
            runtime_config = %config.runtime_config.display(),
            timeout_secs = CONFIG_TEST_TIMEOUT.as_secs(),
            "validating mihomo config"
        );
        let output = timeout(
            CONFIG_TEST_TIMEOUT,
            Command::new(&config.mihomo_binary)
                .arg("-t")
                .arg("-d")
                .arg(&self.paths.runtime_dir)
                .arg("-f")
                .arg(&config.runtime_config)
                .output(),
        )
        .await
        .map_err(|_| CoreManagerError::Timeout("mihomo config validation", CONFIG_TEST_TIMEOUT))?
        .map_err(CoreManagerError::Io)?;

        if output.status.success() {
            info!("mihomo config validation completed");
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if !stderr.is_empty() { stderr } else { stdout };
        self.emit_log(
            "error",
            "core-manager",
            &format!("mihomo config validation failed: {message}"),
        );
        Err(CoreManagerError::Validation(message))
    }

    async fn binary_version(&self, binary: &std::path::Path) -> Option<String> {
        let output = match timeout(VERSION_TIMEOUT, Command::new(binary).arg("-v").output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => {
                warn!("failed to read mihomo version: {err}");
                return None;
            }
            Err(_) => {
                warn!(
                    timeout_secs = VERSION_TIMEOUT.as_secs(),
                    "reading mihomo version timed out"
                );
                return None;
            }
        };
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            None
        } else {
            Some(stdout.lines().next().unwrap_or_default().to_string())
        }
    }

    async fn sync_process_state(
        &self,
        active_profile_id: Option<String>,
        controller_addr: String,
    ) -> Result<(), CoreManagerError> {
        let exited = {
            let mut guard = self.child.lock().await;
            if let Some(mut child) = guard.take() {
                match child.try_wait().map_err(CoreManagerError::Io)? {
                    Some(status) => Some((status.success(), status.code())),
                    None => {
                        *guard = Some(child);
                        None
                    }
                }
            } else {
                None
            }
        };

        if let Some((success, code)) = exited {
            let last_error = Some(if success {
                "mihomo exited unexpectedly with code 0".to_string()
            } else {
                format!("mihomo exited unexpectedly with code {:?}", code)
            });
            let version = self.current_runtime_status().await.version;
            warn!(
                "{}",
                last_error
                    .as_deref()
                    .unwrap_or("mihomo exited unexpectedly")
            );
            self.update_state(RuntimeStatus {
                state: CoreRunState::Error,
                pid: None,
                active_profile_id,
                started_at: None,
                last_error,
                controller_addr,
                version,
            })
            .await;
        }

        Ok(())
    }

    async fn mark_start_error(
        &self,
        active_profile_id: Option<String>,
        controller_addr: String,
        message: String,
    ) {
        self.emit_log("error", "core-manager", &message);
        let version = self.current_runtime_status().await.version;
        self.update_state(RuntimeStatus {
            state: CoreRunState::Error,
            pid: None,
            active_profile_id,
            started_at: None,
            last_error: Some(message),
            controller_addr,
            version,
        })
        .await;
    }

    async fn current_runtime_status(&self) -> RuntimeStatus {
        self.status.read().await.clone()
    }

    async fn update_state(&self, status: RuntimeStatus) {
        {
            let mut guard = self.status.write().await;
            *guard = status.clone();
        }

        let event = ServerEvent::CoreStatus(CoreStatusResponse {
            state: status.state.clone(),
            pid: status.pid,
            active_profile_id: status.active_profile_id.clone(),
            started_at: status.started_at.clone(),
            last_error: status.last_error.clone(),
            controller_addr: status.controller_addr.clone(),
            version: status.version.clone(),
        });
        let _ = self.events.send(event);
    }

    fn emit_log(&self, level: &str, source: &str, message: &str) {
        let formatted = format!("{source}: {message}");
        match level {
            "error" => error!("{formatted}"),
            "warn" => warn!("{formatted}"),
            _ => info!("{formatted}"),
        }

        let _ = self.events.send(ServerEvent::Log(LogEntry {
            ts: now_iso(),
            level: level.to_string(),
            source: source.to_string(),
            message: message.to_string(),
        }));
    }

    fn spawn_log_reader<T>(&self, reader: T, source: String, level: String)
    where
        T: tokio::io::AsyncRead + Send + Unpin + 'static,
    {
        let manager = self.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                manager.emit_log(&level, &source, &line);
            }
        });
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[derive(Debug, Error)]
pub enum CoreManagerError {
    #[error("process io error: {0}")]
    Io(std::io::Error),
    #[error("{0} timed out after {1:?}")]
    Timeout(&'static str, Duration),
    #[error("mihomo config validation failed: {0}")]
    Validation(String),
}
