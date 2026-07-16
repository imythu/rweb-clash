use crate::assets::EmbeddedAssets;
use crate::error::AppError;
use crate::paths::{restrict_sensitive_file_permissions, AppPaths};
use crate::storage::{RuleSetRecord, Storage};
use crate::util::content_hash;
use std::io::ErrorKind;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

const GEOIP_RESOURCE_PATH: &str = "runtime/geoip.metadb";
const MIN_GEOIP_DATABASE_BYTES: u64 = 1_048_576;

pub struct BootstrapOptions<'a> {
    pub packaged_resources: Option<&'a Path>,
    pub embedded_assets: Option<&'static EmbeddedAssets>,
}

pub async fn bootstrap_runtime_assets(
    paths: &AppPaths,
    storage: &Storage,
    options: BootstrapOptions<'_>,
) -> Result<(), AppError> {
    bootstrap_core(paths, options.packaged_resources, options.embedded_assets).await?;
    bootstrap_geoip_database(paths, options.packaged_resources, options.embedded_assets).await?;
    bootstrap_rule_sets(
        paths,
        storage,
        options.packaged_resources,
        options.embedded_assets,
    )
    .await
}

async fn bootstrap_core(
    paths: &AppPaths,
    packaged_resources: Option<&Path>,
    embedded_assets: Option<&'static EmbeddedAssets>,
) -> Result<(), AppError> {
    let target = paths.mihomo_binary();
    if target.is_file() {
        return Ok(());
    }

    let file_name = if cfg!(windows) {
        "mihomo.exe"
    } else {
        "mihomo"
    };
    let resource_path = format!("core/{file_name}");
    if copy_packaged_file(packaged_resources, &resource_path, &target).await? {
        make_executable(&target).await;
        info!(
            mihomo_binary = %AppPaths::display(&target),
            "bootstrapped packaged mihomo core"
        );
        return Ok(());
    }
    if copy_embedded_file(embedded_assets, &resource_path, &target).await? {
        make_executable(&target).await;
        info!(
            mihomo_binary = %AppPaths::display(&target),
            "bootstrapped embedded mihomo core"
        );
        return Ok(());
    }

    warn!(
        expected = %resource_path,
        target = %AppPaths::display(&target),
        "no packaged mihomo core found"
    );
    Ok(())
}

async fn bootstrap_geoip_database(
    paths: &AppPaths,
    packaged_resources: Option<&Path>,
    embedded_assets: Option<&'static EmbeddedAssets>,
) -> Result<(), AppError> {
    let target = paths.profiles_dir.join("geoip.metadb");
    match tokio::fs::symlink_metadata(&target).await {
        Ok(metadata)
            if metadata.file_type().is_file() && metadata.len() >= MIN_GEOIP_DATABASE_BYTES =>
        {
            return Ok(());
        }
        Ok(metadata) if metadata.file_type().is_file() => {
            warn!(
                target = %AppPaths::display(&target),
                bytes = metadata.len(),
                minimum_bytes = MIN_GEOIP_DATABASE_BYTES,
                "discarding incomplete GeoIP database"
            );
            tokio::fs::remove_file(&target).await?;
        }
        Ok(_) => {
            return Err(AppError::internal(format!(
                "GeoIP database path is not a regular file: {}",
                AppPaths::display(&target)
            )));
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(AppError::from(error)),
    }

    let copied = if copy_packaged_geoip(packaged_resources, &target).await? {
        true
    } else {
        copy_embedded_geoip(embedded_assets, &target).await?
    };
    if copied {
        info!(
            target = %AppPaths::display(&target),
            "bootstrapped bundled GeoIP database"
        );
    } else {
        warn!(
            expected = GEOIP_RESOURCE_PATH,
            target = %AppPaths::display(&target),
            "no packaged GeoIP database found"
        );
    }
    Ok(())
}

async fn copy_packaged_geoip(
    packaged_resources: Option<&Path>,
    target: &Path,
) -> Result<bool, AppError> {
    let Some(root) = packaged_resources else {
        return Ok(false);
    };
    let source = root.join(GEOIP_RESOURCE_PATH);
    let metadata = match tokio::fs::metadata(&source).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(AppError::from(error)),
    };
    if !metadata.is_file() || metadata.len() < MIN_GEOIP_DATABASE_BYTES {
        warn!(
            source = %AppPaths::display(&source),
            bytes = metadata.len(),
            minimum_bytes = MIN_GEOIP_DATABASE_BYTES,
            "ignoring invalid packaged GeoIP database"
        );
        return Ok(false);
    }
    copy_file(&source, target).await
}

async fn copy_embedded_geoip(
    embedded_assets: Option<&'static EmbeddedAssets>,
    target: &Path,
) -> Result<bool, AppError> {
    let Some(bytes) = embedded_assets.and_then(|assets| assets.get(GEOIP_RESOURCE_PATH)) else {
        return Ok(false);
    };
    if bytes.len() < MIN_GEOIP_DATABASE_BYTES as usize {
        warn!(
            bytes = bytes.len(),
            minimum_bytes = MIN_GEOIP_DATABASE_BYTES,
            "ignoring invalid embedded GeoIP database"
        );
        return Ok(false);
    }
    write_new_file_atomically(target, bytes).await
}

async fn bootstrap_rule_sets(
    paths: &AppPaths,
    storage: &Storage,
    packaged_resources: Option<&Path>,
    embedded_assets: Option<&'static EmbeddedAssets>,
) -> Result<(), AppError> {
    for rule_set in storage.rule_sets_for_runtime().await? {
        let resource_path = format!("rule-sets/{}.list", rule_set.id);
        let target = paths.rule_sets_dir.join(format!("{}.list", rule_set.id));
        let copied = if target.is_file() {
            false
        } else if copy_packaged_file(packaged_resources, &resource_path, &target).await? {
            true
        } else {
            copy_embedded_file(embedded_assets, &resource_path, &target).await?
        };

        if target.is_file() {
            update_rule_set_local_path(paths, storage, &rule_set, &target, copied).await?;
            if copied {
                info!(
                    rule_set_id = %rule_set.id,
                    target = %AppPaths::display(&target),
                    "bootstrapped packaged rule set"
                );
            }
        }
    }
    Ok(())
}

async fn update_rule_set_local_path(
    paths: &AppPaths,
    storage: &Storage,
    rule_set: &RuleSetRecord,
    target: &Path,
    refresh_snapshot: bool,
) -> Result<(), AppError> {
    let bytes = tokio::fs::read(target).await?;
    let content = String::from_utf8_lossy(&bytes);
    let relative = paths.rule_set_relative_path(&rule_set.id);
    if !refresh_snapshot && rule_set.local_path.as_deref() == Some(relative.as_str()) {
        return Ok(());
    }
    storage
        .update_rule_set_refresh(
            &rule_set.id,
            &relative,
            bytes.len() as u64,
            count_rules(&content),
            &content_hash(&bytes),
            detect_rule_set_format(&content),
            None,
        )
        .await
}

async fn copy_packaged_file(
    packaged_resources: Option<&Path>,
    resource_path: &str,
    target: &Path,
) -> Result<bool, AppError> {
    let Some(root) = packaged_resources else {
        return Ok(false);
    };
    let source = root.join(resource_path);
    if !source.is_file() {
        return Ok(false);
    }
    copy_file(&source, target).await
}

async fn copy_embedded_file(
    embedded_assets: Option<&'static EmbeddedAssets>,
    resource_path: &str,
    target: &Path,
) -> Result<bool, AppError> {
    let Some(assets) = embedded_assets else {
        return Ok(false);
    };
    let Some(bytes) = assets.get(resource_path) else {
        return Ok(false);
    };
    write_new_file_atomically(target, bytes).await
}

async fn copy_file(source: &Path, target: &Path) -> Result<bool, AppError> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let staging = staging_path(target);
    let result = async {
        tokio::fs::copy(source, &staging).await?;
        restrict_sensitive_file_permissions(&staging)?;
        commit_staging_file(&staging, target).await
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&staging).await;
    }
    result
}

async fn write_new_file_atomically(target: &Path, bytes: &[u8]) -> Result<bool, AppError> {
    if target.exists() {
        return Ok(false);
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let staging = staging_path(target);
    let result = async {
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(crate::paths::PRIVATE_FILE_MODE);
        let mut file = options.open(&staging).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);
        restrict_sensitive_file_permissions(&staging)?;
        commit_staging_file(&staging, target).await
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&staging).await;
    }
    result
}

async fn commit_staging_file(staging: &Path, target: &Path) -> Result<bool, AppError> {
    if target.exists() {
        tokio::fs::remove_file(staging).await?;
        return Ok(false);
    }
    tokio::fs::rename(staging, target).await?;
    restrict_sensitive_file_permissions(target)?;
    Ok(true)
}

fn staging_path(target: &Path) -> std::path::PathBuf {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("runtime-asset");
    target.with_file_name(format!(
        ".{file_name}.{}.tmp",
        uuid::Uuid::new_v4().simple()
    ))
}

async fn make_executable(_path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = tokio::fs::metadata(_path).await {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            let _ = tokio::fs::set_permissions(_path, permissions).await;
        }
    }
}

fn count_rules(content: &str) -> u64 {
    content.lines().filter_map(normalize_rule_set_line).count() as u64
}

fn detect_rule_set_format(content: &str) -> &'static str {
    let Ok(serde_yaml::Value::Mapping(mapping)) =
        serde_yaml::from_str::<serde_yaml::Value>(content)
    else {
        return "text";
    };
    let payload_key = serde_yaml::Value::String("payload".into());
    if mapping
        .get(&payload_key)
        .is_some_and(serde_yaml::Value::is_sequence)
    {
        "yaml"
    } else {
        "text"
    }
}

fn normalize_rule_set_line(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line == "payload:" {
        return None;
    }

    let line = line
        .strip_prefix('-')
        .map(str::trim)
        .unwrap_or(line)
        .trim_matches(|ch| ch == '\'' || ch == '"');

    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bootstrap_core, bootstrap_geoip_database, bootstrap_rule_sets, MIN_GEOIP_DATABASE_BYTES,
    };
    use crate::assets::{EmbeddedAssets, EmbeddedFile};
    use crate::paths::AppPaths;
    use crate::storage::Storage;
    use std::path::PathBuf;
    use std::time::Duration;

    #[tokio::test]
    async fn bootstrap_core_uses_tauri_resource_layout() {
        let temp = TestDir::new("rweb-clash-bootstrap-core");
        let root = temp.path().join("runtime");
        let resources = temp.path().join("resources");
        let core_dir = resources.join("core");
        tokio::fs::create_dir_all(&core_dir).await.unwrap();

        let binary_name = if cfg!(windows) {
            "mihomo.exe"
        } else {
            "mihomo"
        };
        tokio::fs::write(core_dir.join(binary_name), b"mihomo-test")
            .await
            .unwrap();

        let paths = AppPaths::from_root(&root);
        bootstrap_core(&paths, Some(&resources), None)
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(paths.mihomo_binary()).await.unwrap(),
            b"mihomo-test"
        );
    }

    #[tokio::test]
    async fn bootstrap_core_uses_embedded_resource_layout() {
        let temp = TestDir::new("rweb-clash-bootstrap-embedded-core");
        let root = temp.path().join("runtime");
        let paths = AppPaths::from_root(&root);

        bootstrap_core(&paths, None, Some(&EMBEDDED_CORE_ASSETS))
            .await
            .unwrap();

        assert_eq!(
            tokio::fs::read(paths.mihomo_binary()).await.unwrap(),
            b"mihomo-embedded"
        );
    }

    #[tokio::test]
    async fn bootstrap_geoip_repairs_incomplete_data_without_replacing_valid_data() {
        let temp = TestDir::new("rweb-clash-bootstrap-geoip");
        let paths = AppPaths::from_root(temp.path().join("runtime"));

        bootstrap_geoip_database(&paths, None, Some(&EMBEDDED_GEOIP_ASSETS))
            .await
            .unwrap();
        let target = paths.profiles_dir.join("geoip.metadb");
        assert_eq!(
            tokio::fs::read(&target).await.unwrap(),
            EMBEDDED_GEOIP_BYTES
        );

        let user_data = vec![0x55; MIN_GEOIP_DATABASE_BYTES as usize + 1];
        tokio::fs::write(&target, &user_data).await.unwrap();
        bootstrap_geoip_database(&paths, None, Some(&EMBEDDED_GEOIP_ASSETS))
            .await
            .unwrap();
        assert_eq!(tokio::fs::read(&target).await.unwrap(), user_data);

        tokio::fs::write(&target, []).await.unwrap();
        bootstrap_geoip_database(&paths, None, Some(&EMBEDDED_GEOIP_ASSETS))
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read(&target).await.unwrap(),
            EMBEDDED_GEOIP_BYTES
        );

        let mut entries = tokio::fs::read_dir(&paths.profiles_dir).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            assert!(!entry.file_name().to_string_lossy().ends_with(".tmp"));
        }
    }

    #[tokio::test]
    async fn bootstrap_geoip_prefers_packaged_data_and_rejects_a_directory_target() {
        let temp = TestDir::new("rweb-clash-bootstrap-packaged-geoip");
        let paths = AppPaths::from_root(temp.path().join("runtime"));
        let resources = temp.path().join("resources");
        let packaged = resources.join("runtime/geoip.metadb");
        tokio::fs::create_dir_all(packaged.parent().unwrap())
            .await
            .unwrap();
        let packaged_data = vec![0x33; MIN_GEOIP_DATABASE_BYTES as usize];
        tokio::fs::write(&packaged, &packaged_data).await.unwrap();

        bootstrap_geoip_database(&paths, Some(&resources), Some(&EMBEDDED_GEOIP_ASSETS))
            .await
            .unwrap();
        let target = paths.profiles_dir.join("geoip.metadb");
        assert_eq!(tokio::fs::read(&target).await.unwrap(), packaged_data);

        tokio::fs::remove_file(&target).await.unwrap();
        tokio::fs::create_dir(&target).await.unwrap();
        let error =
            bootstrap_geoip_database(&paths, Some(&resources), Some(&EMBEDDED_GEOIP_ASSETS))
                .await
                .expect_err("a directory must not be accepted as a GeoIP database");
        assert!(error.message.contains("not a regular file"));
        assert!(target.is_dir());
    }

    #[tokio::test]
    async fn restored_embedded_rule_snapshot_is_marked_fresh() {
        let temp = TestDir::new("rweb-clash-bootstrap-rule-set");
        let paths = AppPaths::from_root(temp.path().join("runtime"));
        paths.ensure_dirs().unwrap();
        let storage = Storage::connect(&paths).await.unwrap();
        let id = "rs_bootstrap_refresh_test";
        storage
            .create_rule_set(
                id,
                "bootstrap-refresh-test",
                "https://example.com/rules",
                86_400,
                None,
                "text",
            )
            .await
            .unwrap();
        let target = paths.rule_sets_dir.join(format!("{id}.list"));
        tokio::fs::write(&target, b"old.example").await.unwrap();
        storage
            .update_rule_set_refresh(
                id,
                &paths.rule_set_relative_path(id),
                11,
                1,
                "old-hash",
                "text",
                None,
            )
            .await
            .unwrap();
        let previous_update = storage
            .list_rule_sets()
            .await
            .unwrap()
            .into_iter()
            .find(|rule_set| rule_set.id == id)
            .and_then(|rule_set| rule_set.last_update)
            .unwrap();
        tokio::fs::remove_file(&target).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        bootstrap_rule_sets(&paths, &storage, None, Some(&EMBEDDED_RULE_SET_ASSETS))
            .await
            .unwrap();

        assert_eq!(tokio::fs::read(&target).await.unwrap(), b"new.example");
        let restored_update = storage
            .list_rule_sets()
            .await
            .unwrap()
            .into_iter()
            .find(|rule_set| rule_set.id == id)
            .and_then(|rule_set| rule_set.last_update)
            .unwrap();
        assert_ne!(restored_update, previous_update);
        let due = storage.due_rule_set_ids().await.unwrap();
        assert!(
            !due.iter().any(|due_id| due_id == id),
            "restored update {restored_update} was unexpectedly due: {due:?}"
        );
    }

    #[cfg(windows)]
    static EMBEDDED_CORE_FILES: &[EmbeddedFile] = &[EmbeddedFile {
        path: "core/mihomo.exe",
        bytes: b"mihomo-embedded",
    }];

    #[cfg(not(windows))]
    static EMBEDDED_CORE_FILES: &[EmbeddedFile] = &[EmbeddedFile {
        path: "core/mihomo",
        bytes: b"mihomo-embedded",
    }];

    static EMBEDDED_CORE_ASSETS: EmbeddedAssets = EmbeddedAssets {
        files: EMBEDDED_CORE_FILES,
    };

    static EMBEDDED_GEOIP_BYTES: [u8; MIN_GEOIP_DATABASE_BYTES as usize] =
        [0x42; MIN_GEOIP_DATABASE_BYTES as usize];

    static EMBEDDED_GEOIP_FILES: &[EmbeddedFile] = &[EmbeddedFile {
        path: "runtime/geoip.metadb",
        bytes: &EMBEDDED_GEOIP_BYTES,
    }];

    static EMBEDDED_GEOIP_ASSETS: EmbeddedAssets = EmbeddedAssets {
        files: EMBEDDED_GEOIP_FILES,
    };

    static EMBEDDED_RULE_SET_FILES: &[EmbeddedFile] = &[EmbeddedFile {
        path: "rule-sets/rs_bootstrap_refresh_test.list",
        bytes: b"new.example",
    }];

    static EMBEDDED_RULE_SET_ASSETS: EmbeddedAssets = EmbeddedAssets {
        files: EMBEDDED_RULE_SET_FILES,
    };

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
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
