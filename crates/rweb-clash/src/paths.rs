use std::path::{Path, PathBuf};

pub(crate) const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
pub(crate) const PRIVATE_FILE_MODE: u32 = 0o600;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root_dir: PathBuf,
    pub data_dir: PathBuf,
    pub profiles_dir: PathBuf,
    pub rule_sets_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub cache_core_dir: PathBuf,
    pub database_file: PathBuf,
    pub runtime_yaml: PathBuf,
    pub frontend_dist: PathBuf,
    secure_root_dir: bool,
}

impl AppPaths {
    pub fn discover() -> std::io::Result<Self> {
        if let Some(root_dir) =
            std::env::var_os("RWEB_CLASH_ROOT").filter(|value| !value.is_empty())
        {
            return Ok(Self::from_root(root_dir));
        }
        let root_dir = std::env::current_dir()?;
        Ok(Self::from_root_inner(root_dir, false))
    }

    pub fn from_root(root_dir: impl Into<PathBuf>) -> Self {
        Self::from_root_inner(root_dir, true)
    }

    fn from_root_inner(root_dir: impl Into<PathBuf>, secure_root_dir: bool) -> Self {
        let root_dir = root_dir.into();
        let data_dir = root_dir.join("data");
        let profiles_dir = data_dir.join("profiles");
        let rule_sets_dir = profiles_dir.join("rule-sets");
        let logs_dir = data_dir.join("logs");
        let cache_core_dir = root_dir.join("cache-core");
        let database_file = data_dir.join("app.db");
        let runtime_yaml = profiles_dir.join("runtime.yaml");
        let frontend_dist = root_dir.join("web").join("dist");

        Self {
            root_dir,
            data_dir,
            profiles_dir,
            rule_sets_dir,
            logs_dir,
            cache_core_dir,
            database_file,
            runtime_yaml,
            frontend_dist,
            secure_root_dir,
        }
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        if self.secure_root_dir {
            ensure_private_directory(&self.root_dir)?;
        }
        ensure_private_directory(&self.data_dir)?;
        ensure_private_directory(&self.profiles_dir)?;
        self.migrate_legacy_rule_sets_dir()?;
        ensure_private_directory(&self.rule_sets_dir)?;
        ensure_private_directory(&self.logs_dir)?;
        std::fs::create_dir_all(&self.cache_core_dir)?;
        restrict_sensitive_file_permissions(&self.database_file)?;
        for suffix in ["-wal", "-shm", "-journal"] {
            restrict_sensitive_file_permissions(&sqlite_companion_path(
                &self.database_file,
                suffix,
            ))?;
        }
        restrict_sensitive_file_permissions(&self.runtime_yaml)?;
        restrict_sensitive_file_permissions(&self.data_dir.join("system-proxy-backup.json"))?;
        Ok(())
    }

    pub fn rule_set_relative_path(&self, id: &str) -> String {
        format!("data/profiles/rule-sets/{id}.list")
    }

    pub fn resolve_local_path(&self, local_path: &str) -> PathBuf {
        let normalized = local_path.replace('\\', "/");
        if let Some(file_name) = normalized.strip_prefix("data/rule-sets/") {
            return self.rule_sets_dir.join(file_name);
        }
        self.root_dir.join(local_path)
    }

    pub fn mihomo_binary(&self) -> PathBuf {
        let exe = if cfg!(windows) {
            "mihomo.exe"
        } else {
            "mihomo"
        };
        self.cache_core_dir.join(exe)
    }

    pub fn display(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    fn legacy_rule_sets_dir(&self) -> PathBuf {
        self.data_dir.join("rule-sets")
    }

    fn migrate_legacy_rule_sets_dir(&self) -> std::io::Result<()> {
        let legacy_dir = self.legacy_rule_sets_dir();
        if legacy_dir == self.rule_sets_dir || !legacy_dir.is_dir() {
            return Ok(());
        }

        if !self.rule_sets_dir.exists() && std::fs::rename(&legacy_dir, &self.rule_sets_dir).is_ok()
        {
            return Ok(());
        }

        std::fs::create_dir_all(&self.rule_sets_dir)?;
        for entry in std::fs::read_dir(&legacy_dir)? {
            let entry = entry?;
            let target = self.rule_sets_dir.join(entry.file_name());
            if !target.exists() {
                std::fs::rename(entry.path(), target)?;
            }
        }
        let _ = std::fs::remove_dir(&legacy_dir);
        Ok(())
    }
}

pub(crate) fn ensure_private_directory(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    set_permissions(path, PRIVATE_DIRECTORY_MODE)
}

pub(crate) fn restrict_sensitive_file_permissions(path: &Path) -> std::io::Result<()> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => set_permissions(path, PRIVATE_FILE_MODE),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub(crate) fn sqlite_companion_path(database: &Path, suffix: &str) -> PathBuf {
    let mut path = database.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}

#[cfg(unix)]
fn set_permissions(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_permissions(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::AppPaths;
    use std::path::PathBuf;

    #[test]
    fn rule_sets_live_under_profiles() {
        let paths = AppPaths::from_root("app-root");

        assert_eq!(
            paths.rule_sets_dir,
            PathBuf::from("app-root")
                .join("data")
                .join("profiles")
                .join("rule-sets")
        );
        assert_eq!(
            paths.rule_set_relative_path("rs_1"),
            "data/profiles/rule-sets/rs_1.list"
        );
    }

    #[test]
    fn legacy_rule_set_paths_resolve_to_profiles_dir() {
        let paths = AppPaths::from_root("app-root");

        assert_eq!(
            paths.resolve_local_path("data/rule-sets/rs_1.list"),
            paths.rule_sets_dir.join("rs_1.list")
        );
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dirs_restricts_existing_sensitive_paths() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TestDir::new("rweb-clash-private-paths");
        let paths = AppPaths::from_root(temp.path());
        let backup_path = paths.data_dir.join("system-proxy-backup.json");
        std::fs::create_dir_all(&paths.profiles_dir).unwrap();
        std::fs::create_dir_all(&paths.logs_dir).unwrap();
        for directory in [&paths.data_dir, &paths.profiles_dir, &paths.logs_dir] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(&paths.database_file, b"database").unwrap();
        std::fs::write(&paths.runtime_yaml, b"runtime").unwrap();
        std::fs::write(&backup_path, b"backup").unwrap();
        for file in [&paths.database_file, &paths.runtime_yaml, &backup_path] {
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o644)).unwrap();
        }

        paths.ensure_dirs().unwrap();

        for directory in [
            &paths.root_dir,
            &paths.data_dir,
            &paths.profiles_dir,
            &paths.rule_sets_dir,
            &paths.logs_dir,
        ] {
            assert_eq!(
                std::fs::metadata(directory).unwrap().permissions().mode() & 0o777,
                0o700,
                "{}",
                directory.display()
            );
        }
        for file in [&paths.database_file, &paths.runtime_yaml, &backup_path] {
            assert_eq!(
                std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
                0o600,
                "{}",
                file.display()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn cwd_layout_keeps_repository_mode_but_protects_its_data_tree() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TestDir::new("rweb-clash-cwd-paths");
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let paths = AppPaths::from_root_inner(temp.path(), false);

        paths.ensure_dirs().unwrap();

        assert_eq!(
            std::fs::metadata(temp.path()).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert_eq!(
            std::fs::metadata(&paths.data_dir)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[cfg(unix)]
    struct TestDir {
        path: PathBuf,
    }

    #[cfg(unix)]
    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "{name}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    #[cfg(unix)]
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
