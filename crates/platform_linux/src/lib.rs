use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub app_dir: PathBuf,
    pub bundled_core_dir: PathBuf,
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub scripts_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub database_file: PathBuf,
    pub runtime_config: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let data_dir = env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("data");
        let bundled_core_dir = env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("cache-core");
        let app_dir = data_dir.clone();

        let config_dir = app_dir.join("config");
        let cache_dir = app_dir.join("cache");
        let scripts_dir = app_dir.clone();
        let runtime_dir = config_dir.clone();

        Ok(Self {
            database_file: app_dir.join("rweb-clash.sqlite"),
            runtime_config: runtime_dir.join("config.yaml"),
            data_dir,
            app_dir,
            bundled_core_dir,
            config_dir,
            cache_dir,
            scripts_dir,
            runtime_dir,
        })
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for path in [
            &self.app_dir,
            &self.data_dir,
            &self.bundled_core_dir,
            &self.config_dir,
            &self.cache_dir,
        ] {
            fs::create_dir_all(path)
                .with_context(|| format!("failed to create directory {}", path.display()))?;
        }
        Ok(())
    }

    pub fn relative_to_app(&self, value: &str) -> PathBuf {
        self.app_dir.join(value)
    }

    pub fn bundled_mihomo_binary(&self) -> PathBuf {
        self.bundled_core_dir.join(Self::mihomo_binary_name())
    }

    pub fn mihomo_binary_name() -> &'static str {
        #[cfg(windows)]
        {
            "mihomo.exe"
        }

        #[cfg(not(windows))]
        {
            "mihomo"
        }
    }

    pub fn resolve_mihomo_binary(&self) -> Option<PathBuf> {
        let bundled_path = self.bundled_mihomo_binary();
        if bundled_path.exists() {
            return Some(bundled_path);
        }
        None
    }

    pub fn display_path(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_mihomo_binary_lives_under_current_dir_cache_core() {
        let paths = AppPaths::discover().expect("paths should resolve");
        let expected = env::current_dir().unwrap().join("cache-core");

        assert_eq!(paths.bundled_core_dir, expected);
        assert_eq!(
            paths.bundled_mihomo_binary(),
            expected.join(AppPaths::mihomo_binary_name())
        );
    }

    #[test]
    fn app_data_lives_under_current_dir_data() {
        let paths = AppPaths::discover().expect("paths should resolve");
        let expected = env::current_dir().unwrap().join("data");

        assert_eq!(paths.data_dir, expected);
        assert_eq!(paths.app_dir, expected);
        assert_eq!(paths.database_file, expected.join("rweb-clash.sqlite"));
        assert_eq!(paths.cache_dir, expected.join("cache"));
        assert_eq!(paths.config_dir, expected.join("config"));
        assert_eq!(
            paths.runtime_config,
            expected.join("config").join("config.yaml")
        );
    }
}
