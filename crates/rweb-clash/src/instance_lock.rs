use crate::error::AppError;
use crate::paths::AppPaths;
#[cfg(unix)]
use crate::paths::{PRIVATE_DIRECTORY_MODE, PRIVATE_FILE_MODE};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

const DATA_ROOT_LOCK_FILE: &str = "instance.lock";
#[cfg(unix)]
const GLOBAL_APP_LOCK_FILE: &str = "app.lock";
#[cfg(not(unix))]
const WINDOWS_GLOBAL_APP_LOCK_FILE: &str = "rweb-clash-app.lock";

#[derive(Debug)]
pub(crate) struct DataRootLock {
    _file: File,
}

#[derive(Debug)]
pub(crate) struct GlobalAppLock {
    _file: File,
}

impl DataRootLock {
    pub(crate) fn acquire(paths: &AppPaths) -> Result<Self, AppError> {
        let lock_path = paths.data_dir.join(DATA_ROOT_LOCK_FILE);
        let file = acquire_file_lock(
            &lock_path,
            "data_root_in_use",
            format!(
                "another rweb-clash instance is already using data directory {}",
                AppPaths::display(&paths.data_dir)
            ),
            "data directory",
        )?;
        Ok(Self { _file: file })
    }
}

impl GlobalAppLock {
    pub(crate) fn acquire(paths: &AppPaths) -> Result<Self, AppError> {
        let lock_path = global_app_lock_path(paths).map_err(|error| {
            AppError::internal(format!("failed to prepare the global app lock: {error}"))
        })?;
        Self::acquire_at(&lock_path)
    }

    pub(crate) fn acquire_at(lock_path: &Path) -> Result<Self, AppError> {
        let file = acquire_file_lock(
            lock_path,
            "app_instance_in_use",
            "another rweb-clash app instance is already running for this user".into(),
            "global app",
        )?;
        Ok(Self { _file: file })
    }
}

fn acquire_file_lock(
    path: &Path,
    conflict_code: &str,
    conflict_message: String,
    description: &str,
) -> Result<File, AppError> {
    let file = open_lock_file(path).map_err(|error| {
        AppError::internal(format!(
            "failed to open {description} lock {}: {error}",
            AppPaths::display(path)
        ))
    })?;

    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(AppError::conflict(conflict_code, conflict_message)),
        Err(TryLockError::Error(error)) => Err(AppError::internal(format!(
            "failed to acquire {description} lock {}: {error}",
            AppPaths::display(path)
        ))),
    }
}

fn open_lock_file(path: &Path) -> std::io::Result<File> {
    match create_lock_file(path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = std::fs::symlink_metadata(path)?;
            if !metadata.file_type().is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "lock path exists but is not a regular file",
                ));
            }
            OpenOptions::new().read(true).write(true).open(path)
        }
        Err(error) => Err(error),
    }
}

fn create_lock_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(PRIVATE_FILE_MODE);
    }

    options.open(path)
}

#[cfg(windows)]
fn global_app_lock_path(_paths: &AppPaths) -> std::io::Result<PathBuf> {
    Ok(std::env::temp_dir().join(WINDOWS_GLOBAL_APP_LOCK_FILE))
}

#[cfg(unix)]
fn global_app_lock_path(paths: &AppPaths) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::MetadataExt;

    let directory = if let Some(runtime_dir) =
        std::env::var_os("XDG_RUNTIME_DIR").filter(|value| !value.is_empty())
    {
        PathBuf::from(runtime_dir).join("rweb-clash")
    } else if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        PathBuf::from(home).join(".cache").join("rweb-clash")
    } else {
        let uid = std::fs::metadata(&paths.data_dir)?.uid();
        std::env::temp_dir().join(format!("rweb-clash-{uid}"))
    };
    ensure_private_lock_directory(&directory)?;
    Ok(directory.join(GLOBAL_APP_LOCK_FILE))
}

#[cfg(unix)]
fn ensure_private_lock_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(PRIVATE_DIRECTORY_MODE);
    match builder.create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = std::fs::symlink_metadata(path)?;
            if !metadata.file_type().is_dir() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "global lock directory exists but is not a directory",
                ));
            }
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "global lock directory is accessible by other users",
                ));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(any(windows, unix)))]
fn global_app_lock_path(_paths: &AppPaths) -> std::io::Result<PathBuf> {
    Ok(std::env::temp_dir().join(WINDOWS_GLOBAL_APP_LOCK_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn same_data_root_rejects_a_second_instance() {
        let temp = TestDir::new("same-root");
        let paths = prepared_paths(temp.path());
        let _first = DataRootLock::acquire(&paths).expect("acquire first lock");

        let error = DataRootLock::acquire(&paths).expect_err("reject second lock");

        assert_eq!(error.status, StatusCode::CONFLICT);
        assert_eq!(error.code, "data_root_in_use");
    }

    #[test]
    fn dropping_the_guard_allows_reacquiring_the_data_root() {
        let temp = TestDir::new("reacquire");
        let paths = prepared_paths(temp.path());
        {
            let _first = DataRootLock::acquire(&paths).expect("acquire first lock");
        }

        let _second = DataRootLock::acquire(&paths).expect("reacquire released lock");
    }

    #[test]
    fn different_data_roots_can_be_locked_together() {
        let first_temp = TestDir::new("different-root-a");
        let second_temp = TestDir::new("different-root-b");
        let first_paths = prepared_paths(first_temp.path());
        let second_paths = prepared_paths(second_temp.path());

        let _first = DataRootLock::acquire(&first_paths).expect("acquire first root");
        let _second = DataRootLock::acquire(&second_paths).expect("acquire second root");
    }

    #[test]
    fn global_lock_rejects_a_second_app_and_releases_on_drop() {
        let temp = TestDir::new("global-app");
        let lock_path = temp.path().join("test-global-app.lock");
        let first = GlobalAppLock::acquire_at(&lock_path).expect("acquire global app lock");

        let error = GlobalAppLock::acquire_at(&lock_path).expect_err("reject second app");
        assert_eq!(error.status, StatusCode::CONFLICT);
        assert_eq!(error.code, "app_instance_in_use");

        drop(first);
        let _second = GlobalAppLock::acquire_at(&lock_path).expect("reacquire global app lock");
    }

    #[cfg(unix)]
    #[test]
    fn newly_created_lock_file_is_private() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TestDir::new("private-mode");
        let paths = prepared_paths(temp.path());
        let lock_path = paths.data_dir.join(DATA_ROOT_LOCK_FILE);

        let _lock = DataRootLock::acquire(&paths).expect("acquire lock");

        assert_eq!(
            std::fs::metadata(lock_path)
                .expect("read lock metadata")
                .permissions()
                .mode()
                & 0o777,
            PRIVATE_FILE_MODE
        );
    }

    #[cfg(unix)]
    #[test]
    fn existing_lock_symlink_is_rejected_without_changing_its_target() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temp = TestDir::new("lock-symlink");
        let paths = prepared_paths(temp.path());
        let target = paths.data_dir.join("target-file");
        let lock_path = paths.data_dir.join(DATA_ROOT_LOCK_FILE);
        std::fs::write(&target, b"target").expect("write target");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644))
            .expect("set target mode");
        symlink(&target, &lock_path).expect("create lock symlink");

        DataRootLock::acquire(&paths).expect_err("reject lock symlink");

        assert_eq!(
            std::fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    fn prepared_paths(root: &Path) -> AppPaths {
        let paths = AppPaths::from_root(root);
        paths.ensure_dirs().expect("prepare app directories");
        paths
    }

    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "rweb-clash-instance-lock-{label}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
