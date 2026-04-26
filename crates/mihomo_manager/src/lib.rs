use platform_linux::AppPaths;
use shared_types::{LogEntry, MihomoInstalledVersion, ServerEvent};
use thiserror::Error;
use tokio::process::Command;
use tokio::sync::broadcast;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct MihomoManager {
    paths: AppPaths,
    events: broadcast::Sender<ServerEvent>,
}

impl MihomoManager {
    pub fn new(
        paths: AppPaths,
        events: broadcast::Sender<ServerEvent>,
    ) -> Result<Self, MihomoManagerError> {
        Ok(Self { paths, events })
    }

    pub async fn list_installed_versions(
        &self,
    ) -> Result<Vec<MihomoInstalledVersion>, MihomoManagerError> {
        let binary_path = self.paths.bundled_mihomo_binary();
        if !binary_path.exists() {
            self.emit_log(
                "warn",
                &format!(
                    "bundled mihomo binary not found at {}",
                    AppPaths::display_path(&binary_path)
                ),
            );
            return Ok(Vec::new());
        }

        Ok(vec![MihomoInstalledVersion {
            tag: detect_binary_version(&binary_path)
                .await
                .unwrap_or_else(|| "bundled".to_string()),
            asset_name: AppPaths::mihomo_binary_name().to_string(),
            binary_path: AppPaths::display_path(&binary_path),
            downloaded_at: None,
            active: true,
        }])
    }

    fn emit_log(&self, level: &str, message: &str) {
        match level {
            "warn" => warn!(source = "mihomo-manager", "{message}"),
            _ => info!(source = "mihomo-manager", "{message}"),
        }

        let _ = self.events.send(ServerEvent::Log(LogEntry {
            ts: now_iso(),
            level: level.to_string(),
            source: "mihomo-manager".to_string(),
            message: message.to_string(),
        }));
    }
}

async fn detect_binary_version(path: &std::path::Path) -> Option<String> {
    let output = Command::new(path).arg("-v").output().await.ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    combined
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[derive(Debug, Error)]
pub enum MihomoManagerError {
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to inspect bundled mihomo binary")]
    Inspect,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_bundled_binary_returns_empty_list() {
        let root = std::env::temp_dir().join(format!("rweb-clash-test-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths {
            data_dir: root.join("data"),
            app_dir: root.join("data"),
            bundled_core_dir: root.join("cache-core"),
            config_dir: root.join("data").join("config"),
            cache_dir: root.join("data").join("cache"),
            scripts_dir: root.join("data"),
            runtime_dir: root.join("data").join("config"),
            database_file: root.join("data").join("rweb-clash.sqlite"),
            runtime_config: root.join("data").join("config").join("config.yaml"),
        };
        let (events, _) = broadcast::channel(8);
        let manager = MihomoManager::new(paths, events).unwrap();

        assert!(manager.list_installed_versions().await.unwrap().is_empty());
    }
}
