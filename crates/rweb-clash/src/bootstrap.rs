use crate::assets::EmbeddedAssets;
use crate::error::AppError;
use crate::paths::AppPaths;
use crate::storage::{RuleSetRecord, Storage};
use crate::util::content_hash;
use std::path::Path;
use tracing::{info, warn};

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
            update_rule_set_local_path(paths, storage, &rule_set, &target).await?;
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
) -> Result<(), AppError> {
    let bytes = tokio::fs::read(target).await?;
    let content = String::from_utf8_lossy(&bytes);
    let relative = paths.rule_set_relative_path(&rule_set.id);
    if rule_set.local_path.as_deref() == Some(relative.as_str()) {
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
    copy_file(&source, target).await?;
    Ok(true)
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
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(target, bytes).await?;
    Ok(true)
}

async fn copy_file(source: &Path, target: &Path) -> Result<(), AppError> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::copy(source, target).await?;
    Ok(())
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
    use super::bootstrap_core;
    use crate::assets::{EmbeddedAssets, EmbeddedFile};
    use crate::paths::AppPaths;
    use std::path::PathBuf;

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
