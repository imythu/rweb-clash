use crate::error::AppError;
use crate::paths::{restrict_sensitive_file_permissions, AppPaths};
use crate::storage::Storage;
use crate::types::{BackupResponse, WebDavSettingsInput, WebDavSettingsResponse};
use crate::util::{new_id, now_iso};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use axum::http::StatusCode;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use futures_util::StreamExt;
use reqwest::{Method, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use tracing::{info, warn};
use zip::write::SimpleFileOptions;

const BACKUP_FORMAT_VERSION: u32 = 1;
const BACKUP_SCHEMA_VERSION: u32 = 2;
const MAX_BACKUP_BYTES: usize = 512 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const WEBDAV_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone)]
pub struct BackupService {
    storage: Storage,
    paths: AppPaths,
    operation: Arc<Mutex<()>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct StoredWebDavSettings {
    endpoint: String,
    username: String,
    encrypted_password: Option<String>,
    remote_path: String,
    enabled: bool,
    auto_sync: bool,
    interval_hours: u64,
    retention: usize,
    last_sync: Option<String>,
    last_error: Option<String>,
}

impl Default for StoredWebDavSettings {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            username: String::new(),
            encrypted_password: None,
            remote_path: "rweb-clash".into(),
            enabled: false,
            auto_sync: false,
            interval_hours: 24,
            retention: 7,
            last_sync: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone)]
struct WebDavCredentials {
    endpoint: String,
    username: String,
    password: Option<String>,
    remote_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BackupManifest {
    format_version: u32,
    schema_version: u32,
    app_version: String,
    created_at: String,
    database: ManifestFile,
    rule_sets: Vec<ManifestFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManifestFile {
    path: String,
    size: u64,
    sha256: String,
}

struct ExtractedBackup {
    root: PathBuf,
    database: PathBuf,
    rule_sets: Vec<PathBuf>,
}

impl Drop for ExtractedBackup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl BackupService {
    pub fn new(storage: Storage, paths: AppPaths) -> Self {
        Self {
            storage,
            paths,
            operation: Arc::new(Mutex::new(())),
        }
    }

    pub async fn settings(&self) -> Result<WebDavSettingsResponse, AppError> {
        let _operation = self.operation.lock().await;
        Ok(settings_response(&self.load_settings().await?))
    }

    pub async fn save_settings(
        &self,
        input: WebDavSettingsInput,
    ) -> Result<WebDavSettingsResponse, AppError> {
        let _operation = self.operation.lock().await;
        validate_settings(&input)?;
        let mut stored = self.load_settings().await?;
        stored.endpoint = input.endpoint.trim().trim_end_matches('/').to_string();
        stored.username = input.username.trim().to_string();
        stored.remote_path = normalize_remote_path(&input.remote_path)?;
        stored.enabled = input.enabled;
        stored.auto_sync = input.auto_sync;
        stored.interval_hours = input.interval_hours.clamp(1, 24 * 30);
        stored.retention = input.retention.clamp(1, 100);
        if let Some(password) = input.password {
            stored.encrypted_password = if password.is_empty() {
                None
            } else {
                Some(self.encrypt_password(&password).await?)
            };
        }
        self.write_settings(&stored).await?;
        Ok(settings_response(&stored))
    }

    pub async fn test_webdav(&self) -> Result<(), AppError> {
        let _operation = self.operation.lock().await;
        let settings = self.load_settings().await?;
        let credentials = self.credentials(&settings).await?;
        let client = webdav_client()?;
        let collection = ensure_webdav_collection(&client, &credentials).await?;
        let response = authorized(
            client.request(
                Method::from_bytes(b"PROPFIND").expect("valid WebDAV method"),
                collection,
            ),
            &credentials,
        )
        .header("Depth", "0")
        .send()
        .await
        .map_err(webdav_request_error)?;
        if response.status().is_success() || response.status().as_u16() == 207 {
            Ok(())
        } else {
            Err(webdav_status_error("test", response.status()))
        }
    }

    pub async fn list_backups(&self) -> Result<Vec<BackupResponse>, AppError> {
        let _operation = self.operation.lock().await;
        self.list_backups_locked().await
    }

    pub async fn create_backup(&self) -> Result<BackupResponse, AppError> {
        let _operation = self.operation.lock().await;
        self.create_backup_locked().await
    }

    pub async fn delete_backup(&self, name: &str) -> Result<(), AppError> {
        let _operation = self.operation.lock().await;
        let path = self.backup_path(name)?;
        tokio::fs::remove_file(path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                AppError::not_found("backup_not_found", format!("backup {name} not found"))
            } else {
                error.into()
            }
        })
    }

    pub async fn sync_to_webdav(&self) -> Result<BackupResponse, AppError> {
        let _operation = self.operation.lock().await;
        let backup = self.create_backup_locked().await?;
        let result = self.upload_locked(&backup.name).await;
        self.record_sync_result(&result).await?;
        result?;
        Ok(BackupResponse {
            remote_available: true,
            ..backup
        })
    }

    pub async fn restore_local(&self, name: &str) -> Result<(), AppError> {
        let _operation = self.operation.lock().await;
        let archive = self.backup_path(name)?;
        self.restore_archive_locked(&archive).await
    }

    pub async fn restore_webdav(&self) -> Result<(), AppError> {
        let _operation = self.operation.lock().await;
        let result = async {
            let settings = self.load_settings().await?;
            let credentials = self.credentials(&settings).await?;
            let client = webdav_client()?;
            let collection = collection_url(&credentials)?;
            let latest = child_url(&collection, "latest.zip")?;
            let response = authorized(client.get(latest), &credentials)
                .send()
                .await
                .map_err(webdav_request_error)?;
            if !response.status().is_success() {
                return Err(webdav_status_error("download", response.status()));
            }
            let temporary = self
                .paths
                .backups_dir
                .join(format!(".webdav-restore-{}.zip", new_id("download")));
            let mut stream = response.bytes_stream();
            let mut file = tokio::fs::File::create(&temporary).await?;
            let mut total = 0usize;
            use tokio::io::AsyncWriteExt;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(webdav_request_error)?;
                total = total.saturating_add(chunk.len());
                if total > MAX_BACKUP_BYTES {
                    let _ = tokio::fs::remove_file(&temporary).await;
                    return Err(AppError::new(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "backup_too_large",
                        "remote backup exceeds the size limit",
                    ));
                }
                file.write_all(&chunk).await?;
            }
            file.flush().await?;
            drop(file);
            let restore = self.restore_archive_locked(&temporary).await;
            let _ = tokio::fs::remove_file(&temporary).await;
            restore
        }
        .await;
        self.record_sync_result(&result).await?;
        result
    }

    pub async fn auto_sync_due(&self) -> Result<bool, AppError> {
        let _operation = self.operation.lock().await;
        let settings = self.load_settings().await?;
        if !settings.enabled || !settings.auto_sync {
            return Ok(false);
        }
        let backups = self.list_backups_locked().await?;
        let Some(latest) = backups.first() else {
            return Ok(true);
        };
        let metadata = tokio::fs::metadata(self.backup_path(&latest.name)?).await?;
        let elapsed = metadata
            .modified()
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .unwrap_or(Duration::MAX);
        Ok(elapsed >= Duration::from_secs(settings.interval_hours.saturating_mul(3600)))
    }

    async fn create_backup_locked(&self) -> Result<BackupResponse, AppError> {
        let timestamp = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
        let name = format!("rweb-clash-{timestamp}.zip");
        let target = self.paths.backups_dir.join(&name);
        let temporary_archive = self
            .paths
            .backups_dir
            .join(format!(".{name}.{}.tmp", new_id("archive")));
        let database = self
            .paths
            .backups_dir
            .join(format!(".snapshot-{}.db", new_id("backup")));
        self.storage.backup_database(&database).await?;
        let rule_sets = managed_rule_set_files(&self.paths.rule_sets_dir).await?;
        let created_at = now_iso();
        let archive_created_at = created_at.clone();
        let archive_result = tokio::task::spawn_blocking({
            let database = database.clone();
            let temporary_archive = temporary_archive.clone();
            move || {
                write_backup_archive(
                    &temporary_archive,
                    &database,
                    &rule_sets,
                    &archive_created_at,
                )
            }
        })
        .await
        .map_err(|error| AppError::internal(format!("backup worker failed: {error}")))?;
        let _ = tokio::fs::remove_file(&database).await;
        archive_result?;
        tokio::fs::rename(&temporary_archive, &target).await?;
        restrict_sensitive_file_permissions(&target)?;
        let size = tokio::fs::metadata(&target).await?.len();
        let settings = self.load_settings().await?;
        self.prune_local_backups(settings.retention).await?;
        info!(backup = %name, size, "application backup created");
        Ok(BackupResponse {
            name,
            size,
            created_at,
            remote_available: false,
        })
    }

    async fn list_backups_locked(&self) -> Result<Vec<BackupResponse>, AppError> {
        let mut entries = tokio::fs::read_dir(&self.paths.backups_dir).await?;
        let mut backups = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if !valid_backup_name(&name) || !entry.file_type().await?.is_file() {
                continue;
            }
            let path = entry.path();
            let size = entry.metadata().await?.len();
            let created_at = tokio::task::spawn_blocking(move || read_manifest(&path))
                .await
                .ok()
                .and_then(Result::ok)
                .map(|manifest| manifest.created_at)
                .unwrap_or_else(now_iso);
            backups.push(BackupResponse {
                name,
                size,
                created_at,
                remote_available: false,
            });
        }
        backups.sort_by(|left, right| right.name.cmp(&left.name));
        Ok(backups)
    }

    async fn prune_local_backups(&self, retention: usize) -> Result<(), AppError> {
        let backups = self.list_backups_locked().await?;
        for backup in backups.into_iter().skip(retention.clamp(1, 100)) {
            if let Err(error) =
                tokio::fs::remove_file(self.paths.backups_dir.join(&backup.name)).await
            {
                warn!(backup = %backup.name, %error, "failed to prune an old backup");
            }
        }
        Ok(())
    }

    async fn upload_locked(&self, name: &str) -> Result<(), AppError> {
        let settings = self.load_settings().await?;
        let credentials = self.credentials(&settings).await?;
        let archive = self.backup_path(name)?;
        let bytes = tokio::fs::read(&archive).await?;
        if bytes.len() > MAX_BACKUP_BYTES {
            return Err(AppError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "backup_too_large",
                "backup exceeds the WebDAV upload size limit",
            ));
        }
        let client = webdav_client()?;
        let collection = ensure_webdav_collection(&client, &credentials).await?;
        for remote_name in [name, "latest.zip"] {
            let target = child_url(&collection, remote_name)?;
            let response = authorized(client.put(target), &credentials)
                .header(reqwest::header::CONTENT_TYPE, "application/zip")
                .body(bytes.clone())
                .send()
                .await
                .map_err(webdav_request_error)?;
            if !response.status().is_success() {
                return Err(webdav_status_error("upload", response.status()));
            }
        }
        info!(backup = %name, "application backup uploaded to WebDAV");
        Ok(())
    }

    async fn restore_archive_locked(&self, archive: &Path) -> Result<(), AppError> {
        let extract_root = self
            .paths
            .backups_dir
            .join(format!(".restore-{}", new_id("backup")));
        let archive = archive.to_path_buf();
        let extracted =
            tokio::task::spawn_blocking(move || extract_backup(&archive, &extract_root))
                .await
                .map_err(|error| {
                    AppError::internal(format!("backup restore worker failed: {error}"))
                })??;
        self.storage.restore_database(&extracted.database).await?;
        replace_rule_set_files(&self.paths.rule_sets_dir, &extracted.rule_sets).await?;
        info!("application backup restored");
        Ok(())
    }

    async fn record_sync_result(&self, result: &Result<(), AppError>) -> Result<(), AppError> {
        let mut settings = self.load_settings().await?;
        settings.last_sync = Some(now_iso());
        settings.last_error = result.as_ref().err().map(|error| error.message.clone());
        self.write_settings(&settings).await
    }

    fn backup_path(&self, name: &str) -> Result<PathBuf, AppError> {
        if !valid_backup_name(name) {
            return Err(AppError::bad_request(
                "backup_invalid_name",
                "backup name is invalid",
            ));
        }
        Ok(self.paths.backups_dir.join(name))
    }

    async fn credentials(
        &self,
        settings: &StoredWebDavSettings,
    ) -> Result<WebDavCredentials, AppError> {
        if settings.endpoint.trim().is_empty() {
            return Err(AppError::bad_request(
                "webdav_not_configured",
                "WebDAV endpoint is not configured",
            ));
        }
        Ok(WebDavCredentials {
            endpoint: settings.endpoint.clone(),
            username: settings.username.clone(),
            password: match settings.encrypted_password.as_deref() {
                Some(password) => Some(self.decrypt_password(password).await?),
                None => None,
            },
            remote_path: settings.remote_path.clone(),
        })
    }

    async fn load_settings(&self) -> Result<StoredWebDavSettings, AppError> {
        let path = self.settings_path();
        match tokio::fs::read(&path).await {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(AppError::from),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(StoredWebDavSettings::default())
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn write_settings(&self, settings: &StoredWebDavSettings) -> Result<(), AppError> {
        let path = self.settings_path();
        let temporary = self
            .paths
            .data_dir
            .join(format!(".webdav-settings-{}.tmp", new_id("write")));
        tokio::fs::write(&temporary, serde_json::to_vec_pretty(settings)?).await?;
        restrict_sensitive_file_permissions(&temporary)?;
        if tokio::fs::rename(&temporary, &path).await.is_err() {
            let _ = tokio::fs::remove_file(&path).await;
            tokio::fs::rename(&temporary, &path).await?;
        }
        restrict_sensitive_file_permissions(&path)?;
        Ok(())
    }

    async fn encrypt_password(&self, password: &str) -> Result<String, AppError> {
        let key = self.credential_key().await?;
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(AppError::internal)?;
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).map_err(AppError::internal)?;
        let encrypted = cipher
            .encrypt(Nonce::from_slice(&nonce), password.as_bytes())
            .map_err(|_| AppError::internal("failed to encrypt WebDAV credentials"))?;
        let mut payload = nonce.to_vec();
        payload.extend(encrypted);
        Ok(BASE64.encode(payload))
    }

    async fn decrypt_password(&self, encrypted: &str) -> Result<String, AppError> {
        let payload = BASE64
            .decode(encrypted)
            .map_err(|_| AppError::internal("stored WebDAV credentials are not valid base64"))?;
        if payload.len() <= 12 {
            return Err(AppError::internal(
                "stored WebDAV credentials are truncated",
            ));
        }
        let key = self.credential_key().await?;
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(AppError::internal)?;
        let decrypted = cipher
            .decrypt(Nonce::from_slice(&payload[..12]), &payload[12..])
            .map_err(|_| {
                AppError::new(
                    StatusCode::CONFLICT,
                    "webdav_credentials_unavailable",
                    "stored WebDAV credentials cannot be decrypted on this installation",
                )
            })?;
        String::from_utf8(decrypted).map_err(|_| AppError::internal("WebDAV password is not UTF-8"))
    }

    async fn credential_key(&self) -> Result<[u8; 32], AppError> {
        let path = self.paths.data_dir.join("webdav.key");
        match tokio::fs::read(&path).await {
            Ok(bytes) if bytes.len() == 32 => {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                Ok(key)
            }
            Ok(_) => Err(AppError::internal(
                "WebDAV credential key has an invalid length",
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let mut key = [0u8; 32];
                getrandom::getrandom(&mut key).map_err(AppError::internal)?;
                tokio::fs::write(&path, key).await?;
                restrict_sensitive_file_permissions(&path)?;
                Ok(key)
            }
            Err(error) => Err(error.into()),
        }
    }

    fn settings_path(&self) -> PathBuf {
        self.paths.data_dir.join("webdav.json")
    }
}

fn settings_response(settings: &StoredWebDavSettings) -> WebDavSettingsResponse {
    WebDavSettingsResponse {
        endpoint: settings.endpoint.clone(),
        username: settings.username.clone(),
        password_configured: settings.encrypted_password.is_some(),
        remote_path: settings.remote_path.clone(),
        enabled: settings.enabled,
        auto_sync: settings.auto_sync,
        interval_hours: settings.interval_hours,
        retention: settings.retention,
        last_sync: settings.last_sync.clone(),
        last_error: settings.last_error.clone(),
    }
}

fn validate_settings(input: &WebDavSettingsInput) -> Result<(), AppError> {
    if input.endpoint.trim().is_empty() {
        if input.enabled || input.auto_sync {
            return Err(AppError::bad_request(
                "webdav_invalid",
                "WebDAV endpoint is required when synchronization is enabled",
            ));
        }
        return Ok(());
    }
    let endpoint = Url::parse(input.endpoint.trim()).map_err(|error| {
        AppError::bad_request(
            "webdav_invalid",
            format!("WebDAV endpoint is invalid: {error}"),
        )
    })?;
    if !matches!(endpoint.scheme(), "http" | "https")
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
    {
        return Err(AppError::bad_request(
            "webdav_invalid",
            "WebDAV endpoint must be an HTTP(S) URL without embedded credentials",
        ));
    }
    normalize_remote_path(&input.remote_path)?;
    if input.interval_hours == 0 || input.retention == 0 {
        return Err(AppError::bad_request(
            "webdav_invalid",
            "WebDAV interval and backup retention must be greater than zero",
        ));
    }
    Ok(())
}

fn normalize_remote_path(value: &str) -> Result<String, AppError> {
    let segments = value
        .replace('\\', "/")
        .split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if segments.is_empty()
        || segments.iter().any(|segment| {
            segment == "."
                || segment == ".."
                || segment.len() > 128
                || segment.chars().any(char::is_control)
        })
    {
        return Err(AppError::bad_request(
            "webdav_invalid",
            "WebDAV remote path is invalid",
        ));
    }
    Ok(segments.join("/"))
}

fn webdav_client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(WEBDAV_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(3))
        .no_proxy()
        .build()
        .map_err(AppError::internal)
}

fn authorized(
    request: reqwest::RequestBuilder,
    credentials: &WebDavCredentials,
) -> reqwest::RequestBuilder {
    if credentials.username.is_empty() && credentials.password.is_none() {
        request
    } else {
        request.basic_auth(&credentials.username, credentials.password.as_deref())
    }
}

async fn ensure_webdav_collection(
    client: &reqwest::Client,
    credentials: &WebDavCredentials,
) -> Result<Url, AppError> {
    let mut current = endpoint_url(credentials)?;
    for segment in credentials.remote_path.split('/') {
        current = child_url(&current, segment)?;
        let response = authorized(
            client.request(
                Method::from_bytes(b"MKCOL").expect("valid WebDAV method"),
                current.clone(),
            ),
            credentials,
        )
        .send()
        .await
        .map_err(webdav_request_error)?;
        if !(response.status().is_success() || response.status().as_u16() == 405) {
            return Err(webdav_status_error("create collection", response.status()));
        }
    }
    Ok(current)
}

fn collection_url(credentials: &WebDavCredentials) -> Result<Url, AppError> {
    let mut current = endpoint_url(credentials)?;
    for segment in credentials.remote_path.split('/') {
        current = child_url(&current, segment)?;
    }
    Ok(current)
}

fn endpoint_url(credentials: &WebDavCredentials) -> Result<Url, AppError> {
    let mut url = Url::parse(&credentials.endpoint).map_err(AppError::internal)?;
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

fn child_url(parent: &Url, child: &str) -> Result<Url, AppError> {
    let mut url = parent.clone();
    url.path_segments_mut()
        .map_err(|_| AppError::bad_request("webdav_invalid", "WebDAV URL cannot be a base URL"))?
        .pop_if_empty()
        .push(child)
        .push("");
    if child.contains('.') {
        let path = url.path().trim_end_matches('/').to_string();
        url.set_path(&path);
    }
    Ok(url)
}

fn webdav_request_error(error: reqwest::Error) -> AppError {
    let status = if error.is_timeout() {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    };
    AppError::new(
        status,
        "webdav_request_failed",
        error.without_url().to_string(),
    )
}

fn webdav_status_error(action: &str, status: reqwest::StatusCode) -> AppError {
    AppError::new(
        StatusCode::BAD_GATEWAY,
        "webdav_unexpected_status",
        format!("WebDAV {action} returned {status}"),
    )
}

async fn managed_rule_set_files(directory: &Path) -> Result<Vec<PathBuf>, AppError> {
    let mut entries = tokio::fs::read_dir(directory).await?;
    let mut files = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "list")
        {
            files.push(entry.path());
        }
    }
    files.sort();
    Ok(files)
}

fn write_backup_archive(
    target: &Path,
    database: &Path,
    rule_sets: &[PathBuf],
    created_at: &str,
) -> Result<(), AppError> {
    let database_manifest = manifest_file("database/app.db", database)?;
    let rule_set_manifest = rule_sets
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| AppError::internal("rule-set snapshot has an invalid file name"))?;
            manifest_file(&format!("rule-sets/{name}"), path)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let manifest = BackupManifest {
        format_version: BACKUP_FORMAT_VERSION,
        schema_version: BACKUP_SCHEMA_VERSION,
        app_version: env!("CARGO_PKG_VERSION").into(),
        created_at: created_at.into(),
        database: database_manifest,
        rule_sets: rule_set_manifest,
    };
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o600);
    let file = File::create(target)?;
    let mut archive = zip::ZipWriter::new(file);
    archive.start_file("manifest.json", options)?;
    archive.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    add_archive_file(&mut archive, "database/app.db", database, options)?;
    for path in rule_sets {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        add_archive_file(&mut archive, &format!("rule-sets/{name}"), path, options)?;
    }
    archive.finish()?;
    Ok(())
}

fn add_archive_file(
    archive: &mut zip::ZipWriter<File>,
    name: &str,
    source: &Path,
    options: SimpleFileOptions,
) -> Result<(), AppError> {
    archive.start_file(name, options)?;
    let mut source = File::open(source)?;
    std::io::copy(&mut source, archive)?;
    Ok(())
}

fn manifest_file(path: &str, source: &Path) -> Result<ManifestFile, AppError> {
    Ok(ManifestFile {
        path: path.into(),
        size: std::fs::metadata(source)?.len(),
        sha256: hash_file(source)?,
    })
}

fn hash_file(path: &Path) -> Result<String, AppError> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn read_manifest(path: &Path) -> Result<BackupManifest, AppError> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut manifest = archive.by_name("manifest.json")?;
    if manifest.size() > MAX_MANIFEST_BYTES {
        return Err(AppError::bad_request(
            "backup_invalid",
            "backup manifest exceeds the size limit",
        ));
    }
    let mut bytes = Vec::with_capacity(manifest.size() as usize);
    manifest.read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(AppError::from)
}

fn extract_backup(archive_path: &Path, root: &Path) -> Result<ExtractedBackup, AppError> {
    let manifest = read_manifest(archive_path)?;
    if manifest.format_version != BACKUP_FORMAT_VERSION
        || manifest.schema_version > BACKUP_SCHEMA_VERSION
        || manifest.database.path != "database/app.db"
    {
        return Err(AppError::bad_request(
            "backup_incompatible",
            "backup format or schema version is not supported",
        ));
    }
    std::fs::create_dir_all(root)?;
    let database = root.join("app.db");
    let rules_dir = root.join("rule-sets");
    std::fs::create_dir_all(&rules_dir)?;
    let file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    extract_manifest_file(&mut archive, &manifest.database, &database)?;
    let mut rule_sets = Vec::with_capacity(manifest.rule_sets.len());
    for entry in &manifest.rule_sets {
        let name = Path::new(&entry.path)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| entry.path == format!("rule-sets/{name}") && name.ends_with(".list"))
            .ok_or_else(|| {
                AppError::bad_request("backup_invalid", "backup contains an invalid rule-set path")
            })?;
        let target = rules_dir.join(name);
        extract_manifest_file(&mut archive, entry, &target)?;
        rule_sets.push(target);
    }
    Ok(ExtractedBackup {
        root: root.to_path_buf(),
        database,
        rule_sets,
    })
}

fn extract_manifest_file(
    archive: &mut zip::ZipArchive<File>,
    manifest: &ManifestFile,
    target: &Path,
) -> Result<(), AppError> {
    if manifest.size > MAX_BACKUP_BYTES as u64 {
        return Err(AppError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "backup_too_large",
            "backup entry exceeds the size limit",
        ));
    }
    let mut source = archive.by_name(&manifest.path)?;
    if source.size() != manifest.size {
        return Err(AppError::bad_request(
            "backup_invalid",
            format!("backup entry {} has an unexpected size", manifest.path),
        ));
    }
    let mut destination = File::create(target)?;
    std::io::copy(&mut source, &mut destination)?;
    destination.flush()?;
    if hash_file(target)? != manifest.sha256 {
        return Err(AppError::bad_request(
            "backup_invalid",
            format!("backup entry {} failed checksum validation", manifest.path),
        ));
    }
    Ok(())
}

async fn replace_rule_set_files(directory: &Path, sources: &[PathBuf]) -> Result<(), AppError> {
    let mut entries = tokio::fs::read_dir(directory).await?;
    while let Some(entry) = entries.next_entry().await? {
        if entry.file_type().await?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "list")
        {
            tokio::fs::remove_file(entry.path()).await?;
        }
    }
    for source in sources {
        let name = source
            .file_name()
            .ok_or_else(|| AppError::internal("restored rule-set file has no name"))?;
        let target = directory.join(name);
        tokio::fs::copy(source, &target).await?;
        restrict_sensitive_file_permissions(&target)?;
    }
    Ok(())
}

fn valid_backup_name(name: &str) -> bool {
    name.starts_with("rweb-clash-")
        && name.ends_with(".zip")
        && name.len() <= 128
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SystemConfig;

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "rweb-clash-{name}-{}-{}",
                std::process::id(),
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir_all(&path).expect("create backup test directory");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn rejects_remote_path_traversal() {
        assert!(normalize_remote_path("rweb-clash/backups").is_ok());
        assert!(normalize_remote_path("../backups").is_err());
    }

    #[test]
    fn only_accepts_managed_backup_names() {
        assert!(valid_backup_name("rweb-clash-123.zip"));
        assert!(!valid_backup_name("../rweb-clash-123.zip"));
        assert!(!valid_backup_name("unrelated.zip"));
    }

    #[tokio::test]
    async fn local_backup_round_trip_restores_database_and_rule_sets() {
        let temp = TestDir::new("backup-round-trip");
        let paths = AppPaths::from_root(&temp.path);
        paths.ensure_dirs().expect("create application directories");
        let storage = Storage::connect(&paths).await.expect("connect storage");
        let service = BackupService::new(storage.clone(), paths.clone());

        let original_config = SystemConfig {
            mixed_port: 17_890,
            dns_nameservers: vec!["https://backup.example/dns-query".into()],
            ..SystemConfig::default()
        };
        storage
            .save_config(&original_config)
            .await
            .expect("store original config");
        let original_rule_set = paths.rule_sets_dir.join("custom.list");
        tokio::fs::write(&original_rule_set, "example.com\n")
            .await
            .expect("write original rule set");

        let backup = service.create_backup().await.expect("create backup");
        assert!(paths.backups_dir.join(&backup.name).is_file());

        let changed_config = SystemConfig {
            mixed_port: 27_890,
            dns_nameservers: vec!["https://changed.example/dns-query".into()],
            ..original_config.clone()
        };
        storage
            .save_config(&changed_config)
            .await
            .expect("store changed config");
        tokio::fs::write(&original_rule_set, "changed.example\n")
            .await
            .expect("replace original rule set");
        let extra_rule_set = paths.rule_sets_dir.join("extra.list");
        tokio::fs::write(&extra_rule_set, "extra.example\n")
            .await
            .expect("write extra rule set");

        service
            .restore_local(&backup.name)
            .await
            .expect("restore backup");

        let restored_config = storage.load_config().await.expect("load restored config");
        assert_eq!(restored_config.mixed_port, original_config.mixed_port);
        assert_eq!(
            restored_config.dns_nameservers,
            original_config.dns_nameservers
        );
        assert_eq!(
            tokio::fs::read_to_string(&original_rule_set)
                .await
                .expect("read restored rule set"),
            "example.com\n"
        );
        assert!(!extra_rule_set.exists());
    }
}
