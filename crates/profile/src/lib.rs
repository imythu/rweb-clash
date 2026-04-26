use platform_linux::AppPaths;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT, CACHE_CONTROL, CONTENT_DISPOSITION, PRAGMA,
    USER_AGENT,
};
use rusqlite::{params, Connection, OptionalExtension};
use script_engine::{transform_subscription, ScriptLog, SubscriptionInput};
use serde::{Deserialize, Serialize};
use shared_types::{
    ClashBasicConfig, HttpHeaderEntry, ImportFileRequest, ImportUrlRequest, LogEntry,
    ProfileDetailResponse, ProfileKind, ProfilePreviewResponse, ProfileSourceSummary,
    ProfileSummary, ScriptDetailResponse, ScriptSummary, ServerEvent, SystemConfigResponse,
    UpdateProfileRequest, UpdateScriptRequest, UpdateSystemConfigRequest, UpsertScriptRequest,
    MERGED_PROFILE_ID,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;
use tokio::sync::{broadcast, RwLock};
use tokio::time::{timeout, Duration};
use uuid::Uuid;

const MERGED_PROFILE_NAME: &str = "Merged All Profiles";
const DEFAULT_SUBSCRIPTION_USER_AGENT: &str = "rweb-clash/1.0 clash-verge";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub listen_addr: String,
    pub controller_addr: String,
    pub controller_secret: String,
    #[serde(default)]
    pub clash: ClashBasicConfig,
    pub active_profile_id: Option<String>,
    pub profiles: Vec<StoredProfile>,
    pub scripts: Vec<StoredScript>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:31990".into(),
            controller_addr: "127.0.0.1:9090".into(),
            controller_secret: "rweb-clash".into(),
            clash: ClashBasicConfig::default(),
            active_profile_id: None,
            profiles: Vec::new(),
            scripts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredProfile {
    pub id: String,
    pub name: String,
    pub kind: ProfileKind,
    pub source: StoredProfileSource,
    #[serde(default)]
    pub has_custom_name: bool,
    #[serde(default)]
    pub response_name: Option<String>,
    #[serde(default)]
    pub upload: Option<u64>,
    #[serde(default)]
    pub download: Option<u64>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub expire: Option<u64>,
    #[serde(default)]
    pub script_id: Option<String>,
    pub refresh_interval_hours: u8,
    pub rendered_path: String,
    pub subscription_cache_path: Option<String>,
    pub last_refreshed_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StoredProfileSource {
    Url {
        url: String,
        #[serde(default)]
        request_headers: Vec<HttpHeaderEntry>,
    },
    File {
        filename: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredScript {
    pub id: String,
    pub name: String,
    pub file_name: String,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProfileStore {
    paths: AppPaths,
    config: std::sync::Arc<RwLock<AppConfig>>,
    client: reqwest::Client,
    events: Option<broadcast::Sender<ServerEvent>>,
}

impl ProfileStore {
    pub async fn load(paths: AppPaths) -> Result<Self, ProfileError> {
        paths
            .ensure_dirs()
            .map_err(|err| ProfileError::Setup(err.to_string()))?;

        let mut config = load_config_from_sqlite(&paths).await?;
        normalize_app_config(&mut config);

        let store = Self {
            paths,
            config: std::sync::Arc::new(RwLock::new(config)),
            client: reqwest::Client::new(),
            events: None,
        };

        let snapshot = store.snapshot().await;
        store.persist_snapshot(&snapshot).await?;
        Ok(store)
    }

    pub fn with_event_sender(mut self, events: broadcast::Sender<ServerEvent>) -> Self {
        self.events = Some(events);
        self
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub async fn snapshot(&self) -> AppConfig {
        self.config.read().await.clone()
    }

    pub async fn system_config(&self) -> SystemConfigResponse {
        let snapshot = self.snapshot().await;
        SystemConfigResponse {
            clash: snapshot.clash,
        }
    }

    pub async fn update_system_config(
        &self,
        request: UpdateSystemConfigRequest,
    ) -> Result<SystemConfigResponse, ProfileError> {
        if request.clash.external_controller.trim().is_empty() {
            return Err(ProfileError::InvalidInput(
                "external controller cannot be empty".into(),
            ));
        }

        self.update_config(|config| {
            config.controller_addr = request.clash.external_controller.clone();
            config.controller_secret = request.clash.secret.clone();
            config.clash = request.clash;
        })
        .await?;

        Ok(self.system_config().await)
    }

    pub async fn list_profiles(&self) -> Vec<ProfileSummary> {
        let snapshot = self.snapshot().await;
        self.summaries_from_snapshot(&snapshot)
    }

    pub async fn due_profile_ids(&self) -> Vec<String> {
        let snapshot = self.snapshot().await;
        let now = chrono::Utc::now();
        snapshot
            .profiles
            .iter()
            .filter(|profile| {
                let Some(last_refreshed_at) = &profile.last_refreshed_at else {
                    return true;
                };
                let Ok(last_refreshed_at) = chrono::DateTime::parse_from_rfc3339(last_refreshed_at)
                else {
                    return true;
                };
                let elapsed =
                    now.signed_duration_since(last_refreshed_at.with_timezone(&chrono::Utc));
                elapsed.num_seconds() >= i64::from(profile.refresh_interval_hours) * 60 * 60
            })
            .map(|profile| profile.id.clone())
            .collect()
    }

    pub async fn list_scripts(&self) -> Vec<ScriptSummary> {
        let mut scripts = self
            .snapshot()
            .await
            .scripts
            .into_iter()
            .map(|script| ScriptSummary {
                id: script.id,
                name: script.name,
                updated_at: script.updated_at,
            })
            .collect::<Vec<_>>();
        scripts.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        scripts
    }

    pub async fn script_detail(
        &self,
        script_id: &str,
    ) -> Result<ScriptDetailResponse, ProfileError> {
        let snapshot = self.snapshot().await;
        let script = snapshot
            .scripts
            .iter()
            .find(|script| script.id == script_id)
            .cloned()
            .ok_or_else(|| ProfileError::Script(format!("script {script_id} not found")))?;
        let code = tokio::fs::read_to_string(self.paths.scripts_dir.join(&script.file_name))
            .await
            .map_err(ProfileError::Io)?;
        Ok(ScriptDetailResponse {
            id: script.id,
            name: script.name,
            script_code: code,
            updated_at: script.updated_at,
        })
    }

    pub async fn create_script(
        &self,
        request: UpsertScriptRequest,
    ) -> Result<ScriptSummary, ProfileError> {
        if request.name.trim().is_empty() || request.script_code.trim().is_empty() {
            return Err(ProfileError::InvalidInput(
                "script name and code are required".into(),
            ));
        }

        let id = new_id();
        let file_name = format!("{id}.js");
        let path = self.paths.scripts_dir.join(&file_name);
        tokio::fs::write(path, request.script_code).await?;
        let updated_at = Some(now_iso());

        let script = StoredScript {
            id: id.clone(),
            name: request.name.trim().to_string(),
            file_name,
            updated_at: updated_at.clone(),
        };

        self.update_config(move |config| {
            config.scripts.push(script);
        })
        .await?;

        Ok(ScriptSummary {
            id,
            name: request.name.trim().to_string(),
            updated_at,
        })
    }

    pub async fn update_script(
        &self,
        script_id: &str,
        request: UpdateScriptRequest,
    ) -> Result<ScriptSummary, ProfileError> {
        if request.name.is_none() && request.script_code.is_none() {
            return Err(ProfileError::InvalidInput(
                "script name or code is required".into(),
            ));
        }

        let snapshot = self.snapshot().await;
        let script = snapshot
            .scripts
            .iter()
            .find(|script| script.id == script_id)
            .cloned()
            .ok_or_else(|| ProfileError::Script(format!("script {script_id} not found")))?;

        let mut updated_name = script.name.clone();
        let mut updated_at = script.updated_at.clone();
        let mut changed = false;

        if let Some(name) = request.name {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err(ProfileError::InvalidInput(
                    "script name cannot be empty".into(),
                ));
            }
            updated_name = trimmed.to_string();
            changed = true;
        }

        if let Some(script_code) = request.script_code {
            let trimmed = script_code.trim();
            if trimmed.is_empty() {
                return Err(ProfileError::InvalidInput(
                    "script code cannot be empty".into(),
                ));
            }
            tokio::fs::write(self.paths.scripts_dir.join(&script.file_name), trimmed).await?;
            updated_at = Some(now_iso());
            changed = true;
        }

        if changed {
            updated_at = Some(now_iso());
        }

        let script_id = script_id.to_string();
        let updated_name_clone = updated_name.clone();
        let updated_at_clone = updated_at.clone();
        self.update_config(move |config| {
            if let Some(stored) = config
                .scripts
                .iter_mut()
                .find(|script| script.id == script_id)
            {
                stored.name = updated_name_clone;
                stored.updated_at = updated_at_clone;
            }
        })
        .await?;

        Ok(ScriptSummary {
            id: script.id,
            name: updated_name,
            updated_at,
        })
    }

    pub async fn delete_script(&self, script_id: &str) -> Result<(), ProfileError> {
        let snapshot = self.snapshot().await;
        let script = snapshot
            .scripts
            .iter()
            .find(|script| script.id == script_id)
            .cloned()
            .ok_or_else(|| ProfileError::Script(format!("script {script_id} not found")))?;

        let script_id = script_id.to_string();
        self.update_config(move |config| {
            config.scripts.retain(|script| script.id != script_id);
            for profile in &mut config.profiles {
                if profile.script_id.as_deref() == Some(script_id.as_str()) {
                    profile.script_id = None;
                }
            }
        })
        .await?;

        let path = self.paths.scripts_dir.join(script.file_name);
        match tokio::fs::remove_file(path).await {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(ProfileError::Io(err)),
        }

        Ok(())
    }

    pub async fn import_url(
        &self,
        request: ImportUrlRequest,
    ) -> Result<ProfileSummary, ProfileError> {
        if request.url.trim().is_empty() {
            return Err(ProfileError::InvalidInput("profile url is required".into()));
        }

        let requested_name = request.name.trim();
        let has_custom_name = !requested_name.is_empty();
        let initial_name = if has_custom_name {
            requested_name.to_string()
        } else {
            request.url.trim().to_string()
        };
        let script_id = self.resolve_import_script_id(request.script_id).await?;
        let refresh_interval_hours = validate_refresh_interval(request.refresh_interval_hours)?;
        let id = new_id();

        let profile = StoredProfile {
            id: id.clone(),
            name: initial_name,
            kind: ProfileKind::Remote,
            source: StoredProfileSource::Url {
                url: request.url.trim().to_string(),
                request_headers: sanitize_request_headers(request.request_headers)?,
            },
            has_custom_name,
            response_name: None,
            upload: None,
            download: None,
            total: None,
            expire: None,
            script_id,
            refresh_interval_hours,
            rendered_path: format!("config/{id}.yaml"),
            subscription_cache_path: Some(format!("cache/{id}.yaml")),
            last_refreshed_at: None,
            last_error: None,
        };

        self.insert_profile(profile.clone()).await?;
        self.refresh_profile(&profile.id).await
    }

    pub async fn import_file(
        &self,
        request: ImportFileRequest,
    ) -> Result<ProfileSummary, ProfileError> {
        if request.content.trim().is_empty() {
            return Err(ProfileError::InvalidInput(
                "file content is required".into(),
            ));
        }

        let requested_name = request.name.trim();
        let inferred_name = infer_file_profile_name(request.filename.as_deref());
        let has_custom_name = !requested_name.is_empty();
        let profile_name = if has_custom_name {
            requested_name.to_string()
        } else {
            inferred_name.unwrap_or_else(|| "Imported File".to_string())
        };
        let script_id = self.resolve_import_script_id(request.script_id).await?;
        let refresh_interval_hours = validate_refresh_interval(request.refresh_interval_hours)?;

        let id = new_id();
        let cache_rel = format!("cache/{id}.yaml");
        let rendered_rel = format!("config/{id}.yaml");
        let cache_abs = self.paths.relative_to_app(&cache_rel);
        tokio::fs::write(&cache_abs, request.content).await?;

        let profile = StoredProfile {
            id,
            name: profile_name,
            kind: ProfileKind::Local,
            source: StoredProfileSource::File {
                filename: request.filename,
            },
            has_custom_name,
            response_name: None,
            upload: None,
            download: None,
            total: None,
            expire: None,
            script_id,
            refresh_interval_hours,
            rendered_path: rendered_rel,
            subscription_cache_path: Some(cache_rel),
            last_refreshed_at: None,
            last_error: None,
        };

        self.insert_profile(profile.clone()).await?;
        self.refresh_profile(&profile.id).await
    }

    pub async fn refresh_profile(&self, profile_id: &str) -> Result<ProfileSummary, ProfileError> {
        if profile_id == MERGED_PROFILE_ID {
            return self.refresh_merged_profile().await;
        }

        let result = self.refresh_profile_inner(profile_id).await;
        if let Err(err) = &result {
            let _ = self
                .mutate_profile(profile_id, |profile| {
                    profile.last_error = Some(err.to_string());
                })
                .await;
        }
        result
    }

    async fn refresh_profile_inner(
        &self,
        profile_id: &str,
    ) -> Result<ProfileSummary, ProfileError> {
        let mut profile = self.find_profile(profile_id).await?;
        let fetched_at = now_iso();
        let fetched = self.fetch_raw_profile_content(&profile).await?;
        let raw_content = fetched.content;

        if matches!(&profile.source, StoredProfileSource::Url { .. }) {
            profile.upload = fetched.upload;
            profile.download = fetched.download;
            profile.total = fetched.total;
            profile.expire = fetched.expire;
            profile.response_name = fetched.discovered_name.clone();

            if let Some(name) = fetched.discovered_name {
                if !profile.has_custom_name && profile.name != name {
                    profile.name = name;
                }
            }
        }

        if let Some(cache_rel) = &profile.subscription_cache_path {
            let cache_abs = self.paths.relative_to_app(cache_rel);
            tokio::fs::write(cache_abs, &raw_content).await?;
        }

        let rendered = self
            .apply_script_if_needed(&profile, &raw_content, fetched_at.clone())
            .await?;
        let preview_abs = self.preview_path(&profile.id);
        tokio::fs::write(&preview_abs, &rendered).await?;
        parse_runtime_yaml(&rendered)?;

        let rendered_abs = self.paths.relative_to_app(&profile.rendered_path);
        tokio::fs::write(rendered_abs, rendered).await?;

        let refreshed_profile = profile.clone();
        self.mutate_profile(profile_id, move |stored| {
            if matches!(&stored.source, StoredProfileSource::Url { .. }) {
                stored.upload = refreshed_profile.upload;
                stored.download = refreshed_profile.download;
                stored.total = refreshed_profile.total;
                stored.expire = refreshed_profile.expire;
                stored.response_name = refreshed_profile.response_name.clone();
                stored.name = refreshed_profile.name.clone();
            }
            stored.last_refreshed_at = Some(fetched_at.clone());
            stored.last_error = None;
        })
        .await?;

        self.summary_by_id(profile_id).await
    }

    pub async fn activate_profile(&self, profile_id: &str) -> Result<PathBuf, ProfileError> {
        if profile_id == MERGED_PROFILE_ID {
            return self.activate_merged_profile().await;
        }

        let result = self.activate_profile_inner(profile_id).await;
        if let Err(err) = &result {
            let _ = self
                .mutate_profile(profile_id, |profile| {
                    profile.last_error = Some(err.to_string());
                })
                .await;
        }
        result
    }

    async fn activate_profile_inner(&self, profile_id: &str) -> Result<PathBuf, ProfileError> {
        let profile = self.find_profile(profile_id).await?;
        let rendered_abs = self.paths.relative_to_app(&profile.rendered_path);

        if !tokio::fs::try_exists(&rendered_abs).await? {
            self.refresh_profile(profile_id).await?;
        }

        let rendered = tokio::fs::read_to_string(&rendered_abs).await?;
        let mut value = parse_runtime_yaml(&rendered)?;
        let snapshot = self.snapshot().await;
        apply_system_clash_config(&mut value, &snapshot.clash)?;
        self.write_runtime_yaml(&value).await?;

        self.update_config(|config| {
            config.active_profile_id = Some(profile_id.to_string());
        })
        .await?;

        Ok(self.paths.runtime_config.clone())
    }

    async fn refresh_merged_profile(&self) -> Result<ProfileSummary, ProfileError> {
        let merged = self.build_merged_profile().await?;
        let rendered_yaml = serde_yaml::to_string(&merged)?;
        tokio::fs::write(self.preview_path(MERGED_PROFILE_ID), &rendered_yaml).await?;
        tokio::fs::write(self.merged_rendered_path(), rendered_yaml).await?;
        self.summary_by_id(MERGED_PROFILE_ID).await
    }

    async fn activate_merged_profile(&self) -> Result<PathBuf, ProfileError> {
        let mut value = self.build_merged_profile().await?;
        let snapshot = self.snapshot().await;
        apply_system_clash_config(&mut value, &snapshot.clash)?;
        self.write_runtime_yaml(&value).await?;

        self.update_config(|config| {
            config.active_profile_id = Some(MERGED_PROFILE_ID.to_string());
        })
        .await?;

        Ok(self.paths.runtime_config.clone())
    }

    pub async fn ensure_active_runtime_config(&self) -> Result<PathBuf, ProfileError> {
        let snapshot = self.snapshot().await;
        let profile_id = snapshot
            .active_profile_id
            .ok_or_else(|| {
                ProfileError::InvalidInput(
                    "no active profile selected; activate a profile from the Profiles tab before starting mihomo".into(),
                )
            })?;
        self.activate_profile(&profile_id).await
    }

    pub async fn summary_by_id(&self, profile_id: &str) -> Result<ProfileSummary, ProfileError> {
        let snapshot = self.snapshot().await;
        self.summaries_from_snapshot(&snapshot)
            .into_iter()
            .find(|profile| profile.id == profile_id)
            .ok_or(ProfileError::ProfileNotFound(profile_id.to_string()))
    }

    pub async fn profile_detail(
        &self,
        profile_id: &str,
    ) -> Result<ProfileDetailResponse, ProfileError> {
        if profile_id == MERGED_PROFILE_ID {
            let snapshot = self.snapshot().await;
            return Ok(ProfileDetailResponse {
                id: MERGED_PROFILE_ID.into(),
                name: MERGED_PROFILE_NAME.into(),
                kind: ProfileKind::Merged,
                source: ProfileSourceSummary::Merged {
                    description: "Generated from all valid rendered profiles".into(),
                },
                active: snapshot.active_profile_id.as_deref() == Some(MERGED_PROFILE_ID),
                has_custom_name: false,
                upload: None,
                download: None,
                total: None,
                expire: None,
                script_id: None,
                script_name: None,
                refresh_interval_hours: 1,
                last_refreshed_at: None,
                last_error: None,
            });
        }

        let profile = self.find_profile(profile_id).await?;
        let snapshot = self.snapshot().await;
        let script_name = profile
            .script_id
            .as_ref()
            .and_then(|script_id| {
                snapshot
                    .scripts
                    .iter()
                    .find(|script| &script.id == script_id)
            })
            .map(|script| script.name.clone());

        Ok(ProfileDetailResponse {
            id: profile.id.clone(),
            name: profile.name.clone(),
            kind: profile.kind.clone(),
            source: match &profile.source {
                StoredProfileSource::Url {
                    url,
                    request_headers,
                } => ProfileSourceSummary::Url {
                    url: url.clone(),
                    response_name: profile.response_name.clone(),
                    request_headers: request_headers.clone(),
                },
                StoredProfileSource::File { filename } => ProfileSourceSummary::File {
                    filename: filename.clone(),
                },
            },
            active: snapshot.active_profile_id.as_deref() == Some(profile.id.as_str()),
            has_custom_name: profile.has_custom_name,
            upload: profile.upload,
            download: profile.download,
            total: profile.total,
            expire: profile.expire,
            script_id: profile.script_id.clone(),
            script_name,
            refresh_interval_hours: profile.refresh_interval_hours,
            last_refreshed_at: profile.last_refreshed_at.clone(),
            last_error: profile.last_error.clone(),
        })
    }

    pub async fn preview_profile(
        &self,
        profile_id: &str,
    ) -> Result<ProfilePreviewResponse, ProfileError> {
        if profile_id == MERGED_PROFILE_ID {
            return self.preview_merged_profile().await;
        }

        let profile = self.find_profile(profile_id).await?;
        let snapshot = self.snapshot().await;

        let raw_content = match &profile.subscription_cache_path {
            Some(path) => match tokio::fs::read_to_string(self.paths.relative_to_app(path)).await {
                Ok(content) => Some(content),
                Err(err)
                    if err.kind() == std::io::ErrorKind::NotFound
                        && matches!(&profile.source, StoredProfileSource::Url { .. }) =>
                {
                    self.refresh_profile(profile_id).await?;
                    tokio::fs::read_to_string(self.paths.relative_to_app(path))
                        .await
                        .ok()
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                Err(err) => return Err(ProfileError::Io(err)),
            },
            None => None,
        };

        let rendered_path = self.preview_path(&profile.id);
        let rendered_content = match tokio::fs::read_to_string(&rendered_path).await {
            Ok(content) => Some(content),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                match tokio::fs::read_to_string(self.paths.relative_to_app(&profile.rendered_path))
                    .await
                {
                    Ok(content) => Some(content),
                    Err(fallback_err) if fallback_err.kind() == std::io::ErrorKind::NotFound => {
                        None
                    }
                    Err(fallback_err) => return Err(ProfileError::Io(fallback_err)),
                }
            }
            Err(err) => return Err(ProfileError::Io(err)),
        };

        let (root_kind, validation_error, is_valid) = match rendered_content.as_deref() {
            Some(content) => match serde_yaml::from_str::<serde_yaml::Value>(content) {
                Ok(value) => match ensure_root_mapping(&value) {
                    Ok(()) => (Some(yaml_kind(&value).to_string()), None, true),
                    Err(err) => (
                        Some(yaml_kind(&value).to_string()),
                        Some(err.to_string()),
                        false,
                    ),
                },
                Err(err) => (Some("invalid".to_string()), Some(err.to_string()), false),
            },
            None => (
                None,
                profile
                    .last_error
                    .clone()
                    .or_else(|| Some("profile has not been rendered yet".into())),
                false,
            ),
        };

        Ok(ProfilePreviewResponse {
            profile_id: profile.id.clone(),
            profile_name: profile.name.clone(),
            source_summary: match &profile.source {
                StoredProfileSource::Url { url, .. } => url.clone(),
                StoredProfileSource::File { filename } => filename
                    .clone()
                    .unwrap_or_else(|| "inline file import".into()),
            },
            has_custom_name: profile.has_custom_name,
            upload: profile.upload,
            download: profile.download,
            total: profile.total,
            expire: profile.expire,
            script_name: profile
                .script_id
                .as_ref()
                .and_then(|script_id| {
                    snapshot
                        .scripts
                        .iter()
                        .find(|script| &script.id == script_id)
                })
                .map(|script| script.name.clone()),
            refresh_interval_hours: profile.refresh_interval_hours,
            raw_content,
            rendered_content,
            root_kind,
            validation_error,
            is_valid,
        })
    }

    async fn preview_merged_profile(&self) -> Result<ProfilePreviewResponse, ProfileError> {
        let rendered_content =
            match tokio::fs::read_to_string(self.preview_path(MERGED_PROFILE_ID)).await {
                Ok(content) => Some(content),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    let merged = self.build_merged_profile().await?;
                    let content = serde_yaml::to_string(&merged)?;
                    tokio::fs::write(self.preview_path(MERGED_PROFILE_ID), &content).await?;
                    Some(content)
                }
                Err(err) => return Err(ProfileError::Io(err)),
            };

        let (root_kind, validation_error, is_valid) = match rendered_content.as_deref() {
            Some(content) => match serde_yaml::from_str::<serde_yaml::Value>(content) {
                Ok(value) => match ensure_root_mapping(&value) {
                    Ok(()) => (Some(yaml_kind(&value).to_string()), None, true),
                    Err(err) => (
                        Some(yaml_kind(&value).to_string()),
                        Some(err.to_string()),
                        false,
                    ),
                },
                Err(err) => (Some("invalid".to_string()), Some(err.to_string()), false),
            },
            None => (
                None,
                Some("merged profile has not been rendered yet".into()),
                false,
            ),
        };

        Ok(ProfilePreviewResponse {
            profile_id: MERGED_PROFILE_ID.into(),
            profile_name: MERGED_PROFILE_NAME.into(),
            source_summary: "Generated from all valid rendered profiles".into(),
            has_custom_name: false,
            upload: None,
            download: None,
            total: None,
            expire: None,
            script_name: None,
            refresh_interval_hours: 1,
            raw_content: None,
            rendered_content,
            root_kind,
            validation_error,
            is_valid,
        })
    }

    pub async fn update_profile(
        &self,
        profile_id: &str,
        request: UpdateProfileRequest,
    ) -> Result<ProfileSummary, ProfileError> {
        if profile_id == MERGED_PROFILE_ID {
            return Err(ProfileError::InvalidInput(
                "merged profile cannot be edited".into(),
            ));
        }

        let name = request.name.map(|name| name.trim().to_string());
        let url = request.url.map(|url| url.trim().to_string());
        let script_id = request
            .script_id
            .map(|script_id| script_id.trim().to_string());
        let refresh_interval_hours = validate_refresh_interval(request.refresh_interval_hours)?;
        let request_headers = request
            .request_headers
            .map(|headers| sanitize_request_headers(Some(headers)))
            .transpose()?;
        let current = self.find_profile(profile_id).await?;

        if url.as_deref().is_some_and(|url| url.is_empty()) {
            return Err(ProfileError::InvalidInput(
                "profile url cannot be empty".into(),
            ));
        }

        if (url.is_some() || request_headers.is_some())
            && matches!(&current.source, StoredProfileSource::File { .. })
        {
            return Err(ProfileError::InvalidInput(
                "url and request headers can only be updated for remote profiles".into(),
            ));
        }
        if let Some(script_id) = script_id.as_deref() {
            if !script_id.is_empty() {
                self.ensure_script_exists(script_id).await?;
            }
        }

        self.mutate_profile(profile_id, move |profile| {
            if let Some(name) = name {
                if name.is_empty() {
                    profile.has_custom_name = false;
                    profile.name = profile
                        .response_name
                        .clone()
                        .or_else(|| match &profile.source {
                            StoredProfileSource::Url { url, .. } => Some(url.clone()),
                            StoredProfileSource::File { filename } => {
                                infer_file_profile_name(filename.as_deref())
                            }
                        })
                        .unwrap_or_else(|| profile.name.clone());
                } else {
                    profile.name = name;
                    profile.has_custom_name = true;
                }
            }

            if let Some(url) = url {
                match &mut profile.source {
                    StoredProfileSource::Url {
                        url: stored_url, ..
                    } => *stored_url = url,
                    StoredProfileSource::File { .. } => unreachable!(),
                }
            }

            if let Some(request_headers) = request_headers {
                match &mut profile.source {
                    StoredProfileSource::Url {
                        request_headers: stored_headers,
                        ..
                    } => *stored_headers = request_headers,
                    StoredProfileSource::File { .. } => unreachable!(),
                }
            }

            if let Some(script_id) = script_id {
                profile.script_id = if script_id.is_empty() {
                    None
                } else {
                    Some(script_id)
                };
            }
            profile.refresh_interval_hours = refresh_interval_hours;
        })
        .await?;

        self.refresh_profile(profile_id).await
    }

    async fn insert_profile(&self, profile: StoredProfile) -> Result<(), ProfileError> {
        self.update_config(move |config| {
            config.profiles.push(profile);
        })
        .await
    }

    async fn fetch_raw_profile_content(
        &self,
        profile: &StoredProfile,
    ) -> Result<FetchedProfileContent, ProfileError> {
        match &profile.source {
            StoredProfileSource::Url {
                url,
                request_headers,
            } => {
                let response = self
                    .client
                    .get(url)
                    .headers(build_subscription_headers(request_headers)?)
                    .send()
                    .await?;
                let response = response.error_for_status()?;
                let discovered_name = response
                    .headers()
                    .get(CONTENT_DISPOSITION)
                    .and_then(parse_content_disposition_filename);
                let userinfo = response
                    .headers()
                    .get("subscription-userinfo")
                    .and_then(parse_subscription_userinfo);
                let content = response.text().await.map_err(ProfileError::Http)?;
                Ok(FetchedProfileContent {
                    content,
                    discovered_name,
                    upload: userinfo.as_ref().and_then(|info| info.upload),
                    download: userinfo.as_ref().and_then(|info| info.download),
                    total: userinfo.as_ref().and_then(|info| info.total),
                    expire: userinfo.as_ref().and_then(|info| info.expire),
                })
            }
            StoredProfileSource::File { .. } => {
                let cache_rel = profile.subscription_cache_path.as_ref().ok_or_else(|| {
                    ProfileError::InvalidInput("local profile cache is missing".into())
                })?;
                let content = tokio::fs::read_to_string(self.paths.relative_to_app(cache_rel))
                    .await
                    .map_err(ProfileError::Io)?;
                Ok(FetchedProfileContent {
                    content,
                    discovered_name: None,
                    upload: None,
                    download: None,
                    total: None,
                    expire: None,
                })
            }
        }
    }

    async fn apply_script_if_needed(
        &self,
        profile: &StoredProfile,
        raw_content: &str,
        _fetched_at: String,
    ) -> Result<String, ProfileError> {
        let Some(script_id) = &profile.script_id else {
            return Ok(raw_content.to_string());
        };

        let script = match self.read_script(script_id).await {
            Ok(script) => script,
            Err(ProfileError::Script(_)) => return Ok(raw_content.to_string()),
            Err(ProfileError::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(raw_content.to_string())
            }
            Err(err) => return Err(err),
        };
        let config = parse_runtime_yaml(raw_content)?;
        let config = serde_json::to_value(config)
            .map_err(|err| ProfileError::Script(format!("failed to encode script input: {err}")))?;
        let input = SubscriptionInput { config };

        let task = tokio::task::spawn_blocking(move || transform_subscription(&script, input));
        let outcome = timeout(Duration::from_secs(2), task)
            .await
            .map_err(|_| ProfileError::ScriptTimeout)?
            .map_err(|err| ProfileError::Script(err.to_string()))?
            .map_err(|err| ProfileError::Script(err.to_string()))?;
        for log in &outcome.logs {
            emit_script_log(log, profile, script_id, self.events.as_ref());
        }

        serde_yaml::to_string(&outcome.transformed).map_err(ProfileError::Yaml)
    }

    async fn resolve_import_script_id(
        &self,
        script_id: Option<String>,
    ) -> Result<Option<String>, ProfileError> {
        match script_id.map(|id| id.trim().to_string()) {
            Some(script_id) if script_id.is_empty() => Ok(None),
            Some(script_id) => {
                self.ensure_script_exists(&script_id).await?;
                Ok(Some(script_id))
            }
            None => Ok(None),
        }
    }

    async fn ensure_script_exists(&self, script_id: &str) -> Result<(), ProfileError> {
        let snapshot = self.snapshot().await;
        if snapshot.scripts.iter().any(|script| script.id == script_id) {
            Ok(())
        } else {
            Err(ProfileError::InvalidInput(format!(
                "script {script_id} does not exist"
            )))
        }
    }

    async fn read_script(&self, script_id: &str) -> Result<String, ProfileError> {
        let snapshot = self.snapshot().await;
        let script = snapshot
            .scripts
            .iter()
            .find(|script| script.id == script_id)
            .cloned()
            .ok_or_else(|| ProfileError::Script(format!("script {script_id} not found")))?;

        let path = self.paths.scripts_dir.join(script.file_name);
        tokio::fs::read_to_string(path)
            .await
            .map_err(ProfileError::Io)
    }

    async fn build_merged_profile(&self) -> Result<serde_yaml::Value, ProfileError> {
        let snapshot = self.snapshot().await;
        let mut rendered_profiles = Vec::new();

        for profile in snapshot.profiles {
            let rendered_abs = self.paths.relative_to_app(&profile.rendered_path);
            if !tokio::fs::try_exists(&rendered_abs).await? {
                let _ = self.refresh_profile_inner(&profile.id).await;
            }

            let Ok(content) = tokio::fs::read_to_string(&rendered_abs).await else {
                continue;
            };
            let Ok(value) = parse_runtime_yaml(&content) else {
                continue;
            };
            rendered_profiles.push((profile, value));
        }

        merge_rendered_profiles(rendered_profiles)
    }

    async fn write_runtime_yaml(&self, value: &serde_yaml::Value) -> Result<(), ProfileError> {
        let runtime_yaml = serde_yaml::to_string(value)?;
        tokio::fs::write(&self.paths.runtime_config, runtime_yaml).await?;
        Ok(())
    }

    fn merged_rendered_path(&self) -> PathBuf {
        self.paths
            .config_dir
            .join(format!("{MERGED_PROFILE_ID}.yaml"))
    }

    async fn find_profile(&self, profile_id: &str) -> Result<StoredProfile, ProfileError> {
        self.snapshot()
            .await
            .profiles
            .into_iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| ProfileError::ProfileNotFound(profile_id.to_string()))
    }

    async fn mutate_profile<F>(&self, profile_id: &str, mutator: F) -> Result<(), ProfileError>
    where
        F: FnOnce(&mut StoredProfile),
    {
        let profile_id = profile_id.to_string();
        let needle = profile_id.clone();
        self.update_config(move |config| {
            if let Some(profile) = config
                .profiles
                .iter_mut()
                .find(|profile| profile.id == needle)
            {
                mutator(profile);
            }
        })
        .await?;

        if self
            .snapshot()
            .await
            .profiles
            .iter()
            .any(|profile| profile.id == profile_id)
        {
            Ok(())
        } else {
            Err(ProfileError::ProfileNotFound(profile_id))
        }
    }

    async fn update_config<F>(&self, mutator: F) -> Result<(), ProfileError>
    where
        F: FnOnce(&mut AppConfig),
    {
        let snapshot = {
            let mut config = self.config.write().await;
            mutator(&mut config);
            config.clone()
        };

        self.persist_snapshot(&snapshot).await
    }

    async fn persist_snapshot(&self, snapshot: &AppConfig) -> Result<(), ProfileError> {
        persist_config_to_sqlite(&self.paths, snapshot).await
    }

    fn summaries_from_snapshot(&self, snapshot: &AppConfig) -> Vec<ProfileSummary> {
        let mut summaries = Vec::with_capacity(snapshot.profiles.len() + 1);
        summaries.push(merged_profile_summary(snapshot));
        summaries.extend(snapshot.profiles.iter().map(|profile| {
            ProfileSummary {
                id: profile.id.clone(),
                name: profile.name.clone(),
                kind: profile.kind.clone(),
                source: match &profile.source {
                    StoredProfileSource::Url {
                        url,
                        request_headers,
                    } => ProfileSourceSummary::Url {
                        url: url.clone(),
                        response_name: profile.response_name.clone(),
                        request_headers: request_headers.clone(),
                    },
                    StoredProfileSource::File { filename } => ProfileSourceSummary::File {
                        filename: filename.clone(),
                    },
                },
                active: snapshot.active_profile_id.as_deref() == Some(profile.id.as_str()),
                has_custom_name: profile.has_custom_name,
                upload: profile.upload,
                download: profile.download,
                total: profile.total,
                expire: profile.expire,
                script_id: profile.script_id.clone(),
                script_name: profile
                    .script_id
                    .as_ref()
                    .and_then(|script_id| {
                        snapshot
                            .scripts
                            .iter()
                            .find(|script| &script.id == script_id)
                    })
                    .map(|script| script.name.clone()),
                refresh_interval_hours: profile.refresh_interval_hours,
                last_refreshed_at: profile.last_refreshed_at.clone(),
                last_error: profile.last_error.clone(),
            }
        }));
        summaries
    }

    fn preview_path(&self, profile_id: &str) -> PathBuf {
        self.paths
            .data_dir
            .join("cache")
            .join(format!("{profile_id}.preview.yaml"))
    }
}

async fn load_config_from_sqlite(paths: &AppPaths) -> Result<AppConfig, ProfileError> {
    let database_file = paths.database_file.clone();

    tokio::task::spawn_blocking(move || -> Result<AppConfig, ProfileError> {
        if let Some(parent) = database_file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(&database_file)?;
        init_config_table(&connection)?;

        if let Some(content) = connection
            .query_row("select content from app_state where id = 1", [], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
        {
            return Ok(serde_json::from_str::<AppConfig>(&content)?);
        }

        let config = AppConfig::default();
        persist_config_with_connection(&connection, &config)?;
        Ok(config)
    })
    .await
    .map_err(|err| ProfileError::Setup(err.to_string()))?
}

async fn persist_config_to_sqlite(
    paths: &AppPaths,
    snapshot: &AppConfig,
) -> Result<(), ProfileError> {
    let database_file = paths.database_file.clone();
    let snapshot = snapshot.clone();

    tokio::task::spawn_blocking(move || -> Result<(), ProfileError> {
        if let Some(parent) = database_file.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(&database_file)?;
        init_config_table(&connection)?;
        persist_config_with_connection(&connection, &snapshot)?;
        Ok(())
    })
    .await
    .map_err(|err| ProfileError::Setup(err.to_string()))?
}

fn init_config_table(connection: &Connection) -> Result<(), ProfileError> {
    connection.execute_batch(
        "create table if not exists app_state (
            id integer primary key check (id = 1),
            content text not null,
            updated_at text not null
        );",
    )?;
    Ok(())
}

fn persist_config_with_connection(
    connection: &Connection,
    snapshot: &AppConfig,
) -> Result<(), ProfileError> {
    let content = serde_json::to_string_pretty(snapshot)?;
    connection.execute(
        "insert into app_state (id, content, updated_at)
         values (1, ?1, ?2)
         on conflict(id) do update set
            content = excluded.content,
            updated_at = excluded.updated_at",
        params![content, now_iso()],
    )?;
    Ok(())
}

fn normalize_app_config(config: &mut AppConfig) {
    let default_clash = ClashBasicConfig::default();
    if config.clash.external_controller == default_clash.external_controller
        && config.controller_addr != AppConfig::default().controller_addr
    {
        config.clash.external_controller = config.controller_addr.clone();
    }
    if config.clash.secret == default_clash.secret
        && config.controller_secret != AppConfig::default().controller_secret
    {
        config.clash.secret = config.controller_secret.clone();
    }
    config.controller_addr = config.clash.external_controller.clone();
    config.controller_secret = config.clash.secret.clone();
}

fn parse_runtime_yaml(content: &str) -> Result<serde_yaml::Value, ProfileError> {
    let value = serde_yaml::from_str::<serde_yaml::Value>(content)?;
    ensure_root_mapping(&value)?;
    Ok(value)
}

fn apply_system_clash_config(
    value: &mut serde_yaml::Value,
    clash: &ClashBasicConfig,
) -> Result<(), ProfileError> {
    let mapping = value
        .as_mapping_mut()
        .ok_or_else(|| ProfileError::InvalidYaml("profile root must be a YAML object".into()))?;

    mapping.insert(
        yaml_key("external-controller"),
        serde_yaml::Value::String(clash.external_controller.clone()),
    );
    mapping.insert(
        yaml_key("mixed-port"),
        serde_yaml::Value::Number(clash.mixed_port.into()),
    );
    if let Some(port) = clash.port {
        mapping.insert(yaml_key("port"), serde_yaml::Value::Number(port.into()));
    } else {
        mapping.remove(yaml_key("port"));
    }
    if let Some(socks_port) = clash.socks_port {
        mapping.insert(
            yaml_key("socks-port"),
            serde_yaml::Value::Number(socks_port.into()),
        );
    } else {
        mapping.remove(yaml_key("socks-port"));
    }
    mapping.insert(
        yaml_key("allow-lan"),
        serde_yaml::Value::Bool(clash.allow_lan),
    );
    mapping.insert(
        yaml_key("mode"),
        serde_yaml::Value::String(clash_mode_value(&clash.mode).into()),
    );
    mapping.insert(
        yaml_key("log-level"),
        serde_yaml::Value::String(clash_log_level_value(&clash.log_level).into()),
    );
    mapping.insert(yaml_key("ipv6"), serde_yaml::Value::Bool(clash.ipv6));
    if !clash.secret.is_empty() {
        mapping.insert(
            yaml_key("secret"),
            serde_yaml::Value::String(clash.secret.clone()),
        );
    } else {
        mapping.remove(yaml_key("secret"));
    }

    Ok(())
}

fn merge_rendered_profiles(
    profiles: Vec<(StoredProfile, serde_yaml::Value)>,
) -> Result<serde_yaml::Value, ProfileError> {
    if profiles.is_empty() {
        return Err(ProfileError::InvalidInput(
            "no valid profiles are available to merge".into(),
        ));
    }

    let mut merged = profiles
        .iter()
        .find_map(|(_, value)| value.as_mapping().cloned())
        .ok_or_else(|| ProfileError::InvalidYaml("no YAML object profile to merge".into()))?;
    for key in [
        "proxies",
        "proxy-groups",
        "rules",
        "proxy-providers",
        "rule-providers",
    ] {
        merged.remove(yaml_key(key));
    }

    let mut proxy_names = Vec::new();
    let mut seen_proxy_keys = HashSet::new();
    let mut used_names = HashSet::new();
    let mut proxies = Vec::new();

    for (profile, value) in &profiles {
        let Some(items) = sequence_field(value, "proxies") else {
            continue;
        };
        for item in items {
            let proxy_key = serde_yaml::to_string(item)?;
            if !seen_proxy_keys.insert(proxy_key) {
                continue;
            }

            let mut proxy = item.clone();
            let source_name = if profile.name.trim().is_empty() {
                profile.id.as_str()
            } else {
                profile.name.as_str()
            };
            let name = proxy_name(&proxy).unwrap_or_else(|| "Unnamed".into());
            let unique_name = unique_proxy_name(&name, source_name, &mut used_names);
            set_proxy_name(&mut proxy, &unique_name);
            proxy_names.push(unique_name);
            proxies.push(proxy);
        }
    }

    if proxy_names.is_empty() {
        return Err(ProfileError::InvalidInput(
            "merged profile has no proxies".into(),
        ));
    }

    let rules = profiles
        .iter()
        .find_map(|(_, value)| sequence_field(value, "rules").cloned())
        .filter(|rules| !rules.is_empty())
        .unwrap_or_else(|| {
            vec![serde_yaml::Value::String(format!(
                "MATCH,{}",
                merged_global_group_name()
            ))]
        });

    merge_mapping_field(&mut merged, &profiles, "proxy-providers")?;
    merge_mapping_field(&mut merged, &profiles, "rule-providers")?;
    merged.insert(yaml_key("proxies"), serde_yaml::Value::Sequence(proxies));
    merged.insert(
        yaml_key("proxy-groups"),
        serde_yaml::Value::Sequence(build_merged_proxy_groups(&proxy_names)),
    );
    merged.insert(yaml_key("rules"), serde_yaml::Value::Sequence(rules));

    Ok(serde_yaml::Value::Mapping(merged))
}

fn merge_mapping_field(
    merged: &mut serde_yaml::Mapping,
    profiles: &[(StoredProfile, serde_yaml::Value)],
    field: &str,
) -> Result<(), ProfileError> {
    let mut output = serde_yaml::Mapping::new();
    let mut used = HashSet::new();

    for (profile, value) in profiles {
        let Some(mapping) = mapping_field(value, field) else {
            continue;
        };
        for (key, value) in mapping {
            let original = key.as_str().unwrap_or("provider");
            let unique = unique_named_value(original, &profile.name, &mut used);
            output.insert(serde_yaml::Value::String(unique), value.clone());
        }
    }

    if !output.is_empty() {
        merged.insert(yaml_key(field), serde_yaml::Value::Mapping(output));
    }
    Ok(())
}

fn build_merged_proxy_groups(proxy_names: &[String]) -> Vec<serde_yaml::Value> {
    let mut groups = Vec::new();
    let mut all = Vec::with_capacity(proxy_names.len() + 1);
    all.push("DIRECT".to_string());
    all.extend(proxy_names.iter().cloned());

    groups.push(proxy_group(
        merged_global_group_name(),
        "select",
        all.clone(),
        None,
    ));
    groups.push(proxy_group(
        "AUTO",
        "url-test",
        proxy_names.to_vec(),
        Some("http://www.gstatic.com/generate_204"),
    ));

    for (group, keywords) in [
        ("HK", &["hk", "hong kong"][..]),
        ("TW", &["tw", "taiwan"][..]),
        ("JP", &["jp", "japan"][..]),
        ("SG", &["sg", "singapore"][..]),
        ("US", &["us", "usa", "united states"][..]),
    ] {
        let names = proxy_names
            .iter()
            .filter(|name| {
                let lower = name.to_ascii_lowercase();
                keywords.iter().any(|keyword| lower.contains(keyword))
            })
            .cloned()
            .collect::<Vec<_>>();
        if !names.is_empty() {
            groups.push(proxy_group(group, "select", names, None));
        }
    }

    groups.push(proxy_group("OTHERS", "select", proxy_names.to_vec(), None));
    groups
}

fn proxy_group(
    name: &str,
    group_type: &str,
    proxies: Vec<String>,
    url: Option<&str>,
) -> serde_yaml::Value {
    let mut mapping = serde_yaml::Mapping::new();
    mapping.insert(yaml_key("name"), serde_yaml::Value::String(name.into()));
    mapping.insert(
        yaml_key("type"),
        serde_yaml::Value::String(group_type.into()),
    );
    mapping.insert(
        yaml_key("proxies"),
        serde_yaml::Value::Sequence(
            proxies
                .into_iter()
                .map(serde_yaml::Value::String)
                .collect::<Vec<_>>(),
        ),
    );
    if let Some(url) = url {
        mapping.insert(yaml_key("url"), serde_yaml::Value::String(url.into()));
        mapping.insert(yaml_key("interval"), serde_yaml::Value::Number(300.into()));
    }
    serde_yaml::Value::Mapping(mapping)
}

fn sequence_field<'a>(
    value: &'a serde_yaml::Value,
    field: &str,
) -> Option<&'a Vec<serde_yaml::Value>> {
    value.as_mapping()?.get(yaml_key(field))?.as_sequence()
}

fn mapping_field<'a>(value: &'a serde_yaml::Value, field: &str) -> Option<&'a serde_yaml::Mapping> {
    value.as_mapping()?.get(yaml_key(field))?.as_mapping()
}

fn proxy_name(value: &serde_yaml::Value) -> Option<String> {
    value
        .as_mapping()?
        .get(yaml_key("name"))?
        .as_str()
        .map(str::to_string)
}

fn set_proxy_name(value: &mut serde_yaml::Value, name: &str) {
    if let Some(mapping) = value.as_mapping_mut() {
        mapping.insert(yaml_key("name"), serde_yaml::Value::String(name.into()));
    }
}

fn unique_proxy_name(name: &str, source_name: &str, used: &mut HashSet<String>) -> String {
    let trimmed = name.trim();
    let base = if trimmed.is_empty() {
        "Unnamed"
    } else {
        trimmed
    };
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    unique_named_value(base, source_name, used)
}

fn unique_named_value(name: &str, source_name: &str, used: &mut HashSet<String>) -> String {
    let source = source_name.trim();
    let prefix = if source.is_empty() { "Profile" } else { source };
    let mut candidate = format!("{prefix} / {name}");
    if used.insert(candidate.clone()) {
        return candidate;
    }

    let mut index = 2;
    loop {
        candidate = format!("{prefix} / {name} {index}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        index += 1;
    }
}

fn merged_global_group_name() -> &'static str {
    "GLOBAL"
}

fn yaml_key(name: &str) -> serde_yaml::Value {
    serde_yaml::Value::String(name.into())
}

fn merged_profile_summary(snapshot: &AppConfig) -> ProfileSummary {
    ProfileSummary {
        id: MERGED_PROFILE_ID.into(),
        name: MERGED_PROFILE_NAME.into(),
        kind: ProfileKind::Merged,
        source: ProfileSourceSummary::Merged {
            description: "Generated from all valid rendered profiles".into(),
        },
        active: snapshot.active_profile_id.as_deref() == Some(MERGED_PROFILE_ID),
        has_custom_name: false,
        upload: None,
        download: None,
        total: None,
        expire: None,
        script_id: None,
        script_name: None,
        refresh_interval_hours: 1,
        last_refreshed_at: None,
        last_error: None,
    }
}

fn clash_mode_value(mode: &shared_types::ClashMode) -> &'static str {
    match mode {
        shared_types::ClashMode::Rule => "rule",
        shared_types::ClashMode::Global => "global",
        shared_types::ClashMode::Direct => "direct",
    }
}

fn clash_log_level_value(level: &shared_types::ClashLogLevel) -> &'static str {
    match level {
        shared_types::ClashLogLevel::Silent => "silent",
        shared_types::ClashLogLevel::Error => "error",
        shared_types::ClashLogLevel::Warning => "warning",
        shared_types::ClashLogLevel::Info => "info",
        shared_types::ClashLogLevel::Debug => "debug",
    }
}

fn validate_refresh_interval(value: u8) -> Result<u8, ProfileError> {
    if (1..=24).contains(&value) {
        Ok(value)
    } else {
        Err(ProfileError::InvalidInput(
            "refresh interval must be between 1 and 24 hours".into(),
        ))
    }
}

fn emit_script_log(
    log: &ScriptLog,
    profile: &StoredProfile,
    script_id: &str,
    events: Option<&broadcast::Sender<ServerEvent>>,
) {
    match log.level.as_str() {
        "error" => tracing::error!(
            operation = "script.console",
            script_id = %script_id,
            profile_id = %profile.id,
            profile_name = %profile.name,
            message = %log.message,
            "script console.error"
        ),
        "warn" => tracing::warn!(
            operation = "script.console",
            script_id = %script_id,
            profile_id = %profile.id,
            profile_name = %profile.name,
            message = %log.message,
            "script console.warn"
        ),
        "debug" => tracing::debug!(
            operation = "script.console",
            script_id = %script_id,
            profile_id = %profile.id,
            profile_name = %profile.name,
            message = %log.message,
            "script console.debug"
        ),
        "info" => tracing::info!(
            operation = "script.console",
            script_id = %script_id,
            profile_id = %profile.id,
            profile_name = %profile.name,
            message = %log.message,
            "script console.info"
        ),
        _ => tracing::info!(
            operation = "script.console",
            script_id = %script_id,
            profile_id = %profile.id,
            profile_name = %profile.name,
            message = %log.message,
            "script console.log"
        ),
    }

    if let Some(events) = events {
        let _ = events.send(ServerEvent::Log(LogEntry {
            ts: now_iso(),
            level: log.level.clone(),
            source: format!("script:{script_id}"),
            message: format!("{}: {}", profile.name, log.message),
        }));
    }
}

fn ensure_root_mapping(value: &serde_yaml::Value) -> Result<(), ProfileError> {
    if value.as_mapping().is_some() {
        return Ok(());
    }

    Err(ProfileError::InvalidYaml(format!(
        "profile root must be a YAML object, got {}",
        yaml_kind(value)
    )))
}

fn yaml_kind(value: &serde_yaml::Value) -> &'static str {
    match value {
        serde_yaml::Value::Null => "null",
        serde_yaml::Value::Bool(_) => "boolean",
        serde_yaml::Value::Number(_) => "number",
        serde_yaml::Value::String(_) => "string",
        serde_yaml::Value::Sequence(_) => "sequence",
        serde_yaml::Value::Mapping(_) => "object",
        serde_yaml::Value::Tagged(_) => "tagged value",
    }
}

fn sanitize_request_headers(
    headers: Option<Vec<HttpHeaderEntry>>,
) -> Result<Vec<HttpHeaderEntry>, ProfileError> {
    let mut sanitized = Vec::new();
    for header in headers.unwrap_or_default() {
        let name = header.name.trim();
        let value = header.value.trim();
        if name.is_empty() || value.is_empty() {
            return Err(ProfileError::InvalidInput(
                "request header name and value are required".into(),
            ));
        }
        sanitized.push(HttpHeaderEntry {
            name: name.to_string(),
            value: value.to_string(),
        });
    }
    Ok(sanitized)
}

fn build_subscription_headers(
    custom_headers: &[HttpHeaderEntry],
) -> Result<HeaderMap, ProfileError> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(DEFAULT_SUBSCRIPTION_USER_AGENT),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static(
            "application/yaml, application/x-yaml, text/yaml, text/plain, */*",
        ),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(PRAGMA, HeaderValue::from_static("no-cache"));

    for header in custom_headers {
        let name = HeaderName::from_str(header.name.trim()).map_err(|_| {
            ProfileError::InvalidInput(format!("invalid request header name: {}", header.name))
        })?;
        let value = HeaderValue::from_str(header.value.trim()).map_err(|_| {
            ProfileError::InvalidInput(format!("invalid request header value for {}", header.name))
        })?;
        headers.insert(name, value);
    }

    Ok(headers)
}

#[derive(Debug, Clone)]
struct FetchedProfileContent {
    content: String,
    discovered_name: Option<String>,
    upload: Option<u64>,
    download: Option<u64>,
    total: Option<u64>,
    expire: Option<u64>,
}

#[derive(Debug, Clone)]
struct ParsedSubscriptionUserInfo {
    upload: Option<u64>,
    download: Option<u64>,
    total: Option<u64>,
    expire: Option<u64>,
}

fn parse_content_disposition_filename(value: &HeaderValue) -> Option<String> {
    let raw = value.to_str().ok()?;
    let mut filename = None;
    let mut filename_star = None;

    for part in raw.split(';').skip(1) {
        let part = part.trim();
        let (name, value) = match part.split_once('=') {
            Some(pair) => pair,
            None => continue,
        };
        let name = name.trim().to_ascii_lowercase();
        let value = strip_quotes(value.trim());

        match name.as_str() {
            "filename*" => {
                filename_star = parse_extended_filename(value);
            }
            "filename" => {
                filename = parse_simple_filename(value);
            }
            _ => {}
        }
    }

    filename_star.or(filename)
}

fn parse_simple_filename(value: &str) -> Option<String> {
    let unescaped = unescape_quoted_string(value);
    if unescaped.trim().is_empty() {
        None
    } else {
        Some(unescaped)
    }
}

fn parse_extended_filename(value: &str) -> Option<String> {
    let mut parts = value.splitn(3, '\'');
    let charset = parts.next()?.trim().to_ascii_lowercase();
    let _language = parts.next()?;
    let encoded = parts.next()?;
    let bytes = percent_decode(encoded)?;

    match charset.as_str() {
        "utf-8" | "us-ascii" => String::from_utf8(bytes).ok(),
        "iso-8859-1" => Some(bytes.into_iter().map(char::from).collect()),
        _ => None,
    }
}

fn parse_subscription_userinfo(value: &HeaderValue) -> Option<ParsedSubscriptionUserInfo> {
    let raw = value.to_str().ok()?;
    let mut parsed = ParsedSubscriptionUserInfo {
        upload: None,
        download: None,
        total: None,
        expire: None,
    };
    let mut saw_any = false;

    for part in raw.split(';') {
        let part = part.trim();
        let (name, value) = match part.split_once('=') {
            Some(pair) => pair,
            None => continue,
        };

        let name = name.trim().to_ascii_lowercase();
        let value = strip_quotes(value.trim());
        let parsed_value = value.parse::<u64>().ok();

        match name.as_str() {
            "upload" => {
                parsed.upload = parsed_value;
                saw_any = true;
            }
            "download" => {
                parsed.download = parsed_value;
                saw_any = true;
            }
            "total" => {
                parsed.total = parsed_value;
                saw_any = true;
            }
            "expire" => {
                parsed.expire = parsed_value;
                saw_any = true;
            }
            _ => {}
        }
    }

    if saw_any {
        Some(parsed)
    } else {
        None
    }
}

fn strip_quotes(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes.first() == Some(&b'"') && bytes.last() == Some(&b'"') {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn unescape_quoted_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                output.push(next);
            }
        } else {
            output.push(ch);
        }
    }
    output
}

fn percent_decode(input: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(input.len());
    let mut chars = input.as_bytes().iter().copied();
    while let Some(ch) = chars.next() {
        if ch == b'%' {
            let hi = hex_value(chars.next()?)?;
            let lo = hex_value(chars.next()?)?;
            bytes.push((hi << 4) | lo);
        } else {
            bytes.push(ch);
        }
    }
    Some(bytes)
}

fn infer_file_profile_name(filename: Option<&str>) -> Option<String> {
    let filename = filename?.trim();
    if filename.is_empty() {
        return None;
    }

    let trimmed = match filename.rsplit_once('.') {
        Some((base, ext))
            if matches!(ext.to_ascii_lowercase().as_str(), "yaml" | "yml" | "txt") =>
        {
            base.trim()
        }
        _ => filename,
    };
    if trimmed.is_empty() {
        Some(filename.to_string())
    } else {
        Some(trimmed.to_string())
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn new_id() -> String {
    Uuid::new_v4().simple().to_string()
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("profile store setup failed: {0}")]
    Setup(String),
    #[error("profile not found: {0}")]
    ProfileNotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("invalid yaml: {0}")]
    InvalidYaml(String),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("script execution error: {0}")]
    Script(String),
    #[error("subscription script timed out")]
    ScriptTimeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_validation_rejects_invalid_documents() {
        let result = parse_runtime_yaml("port: [");
        assert!(result.is_err());
    }

    #[test]
    fn yaml_validation_rejects_sequence_root() {
        let result = parse_runtime_yaml("- name: test");
        assert!(result.is_err());
    }

    #[test]
    fn subscription_headers_include_defaults_and_allow_override() {
        let headers = build_subscription_headers(&[
            HttpHeaderEntry {
                name: "User-Agent".into(),
                value: "rweb-clash/1.0 clash-verge".into(),
            },
            HttpHeaderEntry {
                name: "X-Token".into(),
                value: "abc".into(),
            },
        ])
        .unwrap();

        assert_eq!(
            headers.get(USER_AGENT).unwrap(),
            "rweb-clash/1.0 clash-verge"
        );
        assert_eq!(headers.get("x-token").unwrap(), "abc");
        assert_eq!(
            headers.get(ACCEPT).unwrap(),
            "application/yaml, application/x-yaml, text/yaml, text/plain, */*"
        );
    }

    #[test]
    fn subscription_headers_default_user_agent_is_rweb_clash() {
        let headers = build_subscription_headers(&[]).unwrap();
        assert_eq!(
            headers.get(USER_AGENT).unwrap(),
            DEFAULT_SUBSCRIPTION_USER_AGENT
        );
    }

    #[test]
    fn content_disposition_prefers_filename_star() {
        let header = HeaderValue::from_static(
            "attachment; filename=\"fallback.yaml\"; filename*=UTF-8''%E6%B5%8B%E8%AF%95.yaml",
        );

        assert_eq!(
            parse_content_disposition_filename(&header).as_deref(),
            Some("测试.yaml")
        );
    }

    #[test]
    fn content_disposition_uses_regular_filename_when_star_is_missing() {
        let header = HeaderValue::from_static("attachment; filename=\"fallback.yaml\"");

        assert_eq!(
            parse_content_disposition_filename(&header).as_deref(),
            Some("fallback.yaml")
        );
    }

    #[test]
    fn subscription_userinfo_parses_values_and_ignores_invalid_entries() {
        let header = HeaderValue::from_static(
            "upload=1024; download=2048; total=4096; expire=1700000000; invalid=abc",
        );

        let parsed = parse_subscription_userinfo(&header).unwrap();
        assert_eq!(parsed.upload, Some(1024));
        assert_eq!(parsed.download, Some(2048));
        assert_eq!(parsed.total, Some(4096));
        assert_eq!(parsed.expire, Some(1700000000));
    }

    #[test]
    fn subscription_userinfo_returns_partial_values_on_parse_failure() {
        let header = HeaderValue::from_static("upload=abc; download=2048");

        let parsed = parse_subscription_userinfo(&header).unwrap();
        assert_eq!(parsed.upload, None);
        assert_eq!(parsed.download, Some(2048));
        assert_eq!(parsed.total, None);
        assert_eq!(parsed.expire, None);
    }

    #[test]
    fn file_profile_name_can_be_inferred_from_filename() {
        assert_eq!(
            infer_file_profile_name(Some("airport-list.YAML")).as_deref(),
            Some("airport-list")
        );
        assert_eq!(
            infer_file_profile_name(Some("plain-name")).as_deref(),
            Some("plain-name")
        );
    }

    #[test]
    fn refresh_interval_must_be_between_one_and_twenty_four_hours() {
        assert_eq!(validate_refresh_interval(1).unwrap(), 1);
        assert_eq!(validate_refresh_interval(24).unwrap(), 24);
        assert!(validate_refresh_interval(0).is_err());
        assert!(validate_refresh_interval(25).is_err());
    }

    #[tokio::test]
    async fn profile_store_persists_config_to_sqlite() {
        let root = std::env::temp_dir().join(format!("rweb-clash-test-{}", new_id()));
        let paths = AppPaths {
            data_dir: root.clone(),
            app_dir: root.clone(),
            bundled_core_dir: root.join("cache-core"),
            config_dir: root.join("config"),
            cache_dir: root.join("cache"),
            scripts_dir: root.clone(),
            runtime_dir: root.join("config"),
            database_file: root.join("rweb-clash.sqlite"),
            runtime_config: root.join("config").join("config.yaml"),
        };

        let store = ProfileStore::load(paths.clone()).await.unwrap();
        store.snapshot().await;

        assert!(paths.database_file.exists());

        let _ = std::fs::remove_dir_all(root);
    }
}
