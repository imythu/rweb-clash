use crate::error::AppError;
use crate::paths::{ensure_private_directory, restrict_sensitive_file_permissions};
use crate::types::SystemConfig;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::Path;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Mutex;

const SYSTEM_PROXY_BACKUP_VERSION: u8 = 1;
static SYSTEM_PROXY_OPERATION: Mutex<()> = Mutex::const_new(());

fn platform_command(program: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new(program);
        command.creation_flags(0x0800_0000);
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new(program)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SystemProxyBackup {
    version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    managed: Option<ManagedProxyEndpoint>,
    #[serde(flatten)]
    state: PlatformProxyState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManagedProxyEndpoint {
    host: String,
    port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "platform", content = "state", rename_all = "snake_case")]
enum PlatformProxyState {
    Windows(WindowsProxyState),
    Macos(MacosProxyState),
    Linux(LinuxProxyState),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct WindowsProxyState {
    proxy_enable: Option<u32>,
    proxy_server: Option<String>,
    proxy_override: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MacosProxyState {
    services: Vec<MacosServiceProxyState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MacosServiceProxyState {
    service: String,
    http: MacosEndpointProxyState,
    https: MacosEndpointProxyState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct MacosEndpointProxyState {
    enabled: bool,
    server: String,
    port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LinuxProxyState {
    mode: String,
    http: LinuxEndpointProxyState,
    https: LinuxEndpointProxyState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LinuxEndpointProxyState {
    host: String,
    port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PlatformRestorePlan {
    Windows(WindowsRestorePlan),
    Macos(Vec<MacosServiceRestorePlan>),
    Linux(LinuxRestorePlan),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WindowsRestorePlan {
    proxy_enable: bool,
    proxy_server: bool,
    proxy_override: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MacosServiceRestorePlan {
    service: String,
    http: MacosEndpointRestorePlan,
    https: MacosEndpointRestorePlan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MacosEndpointRestorePlan {
    original: Option<MacosEndpointProxyState>,
    address: bool,
    enabled: bool,
    current_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LinuxRestorePlan {
    mode: bool,
    http_host: bool,
    http_port: bool,
    https_host: bool,
    https_port: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemProxyRestoreOutcome {
    pub external_changes_preserved: bool,
}

pub async fn apply_system_proxy(config: &SystemConfig, backup_path: &Path) -> Result<(), AppError> {
    let _operation = SYSTEM_PROXY_OPERATION.lock().await;
    let requested = ManagedProxyEndpoint {
        host: "127.0.0.1".into(),
        port: config.mixed_port,
    };
    if config.system_proxy {
        if let Some(backup) = read_backup(backup_path).await? {
            validate_backup_platform(&backup.state)?;
            if managed_endpoint(&backup, &requested) != requested {
                restore_and_remove_backup(backup_path, &backup, &requested).await?;
                create_backup(backup_path, &requested).await?;
            }
        } else {
            create_backup(backup_path, &requested).await?;
        }
        if let Err(enable_error) = enable_system_proxy(&requested.host, requested.port).await {
            let rollback = match read_backup(backup_path).await? {
                Some(backup) => restore_and_remove_backup(backup_path, &backup, &requested)
                    .await
                    .map(|_| ()),
                None => Ok(()),
            };
            return match rollback {
                Ok(()) => Err(enable_error),
                Err(rollback_error) => Err(AppError::internal(format!(
                    "enabling system proxy failed ({enable_error}); partial rollback failed ({rollback_error}); the proxy backup was retained for retry"
                ))),
            };
        }
        Ok(())
    } else if let Some(backup) = read_backup(backup_path).await? {
        restore_and_remove_backup(backup_path, &backup, &requested)
            .await
            .map(|_| ())
    } else {
        clear_owned_system_proxy(&requested.host, requested.port).await
    }
}

pub async fn system_proxy_backup_exists(path: &Path) -> Result<bool, AppError> {
    backup_exists(path).await
}

pub async fn begin_system_proxy_disable(
    config: &SystemConfig,
    backup_path: &Path,
) -> Result<Option<SystemProxyRestoreOutcome>, AppError> {
    let _operation = SYSTEM_PROXY_OPERATION.lock().await;
    let fallback = ManagedProxyEndpoint {
        host: "127.0.0.1".into(),
        port: config.mixed_port,
    };
    match read_backup(backup_path).await? {
        Some(backup) => restore_backup_state(&backup, &fallback).await.map(Some),
        None => {
            clear_owned_system_proxy(&fallback.host, fallback.port).await?;
            Ok(None)
        }
    }
}

pub async fn complete_system_proxy_recovery(backup_path: &Path) -> Result<(), AppError> {
    let _operation = SYSTEM_PROXY_OPERATION.lock().await;
    remove_backup(backup_path).await
}

pub async fn current_system_proxy_url() -> Result<Option<String>, AppError> {
    let state = capture_system_proxy().await?;
    Ok(system_proxy_url_from_state(&state))
}

fn system_proxy_url_from_state(state: &PlatformProxyState) -> Option<String> {
    match state {
        PlatformProxyState::Windows(state) => {
            if state.proxy_enable != Some(1) {
                return None;
            }
            windows_proxy_endpoint(state.proxy_server.as_deref()?)
        }
        PlatformProxyState::Macos(state) => state.services.iter().find_map(|service| {
            [&service.https, &service.http]
                .into_iter()
                .find(|endpoint| {
                    enabled_endpoint(endpoint.enabled, &endpoint.server, endpoint.port)
                })
                .and_then(|endpoint| proxy_url(&endpoint.server, endpoint.port))
        }),
        PlatformProxyState::Linux(state) => {
            if state.mode != "manual" {
                return None;
            }
            [&state.https, &state.http]
                .into_iter()
                .find(|endpoint| !endpoint.host.trim().is_empty() && endpoint.port > 0)
                .and_then(|endpoint| proxy_url(&endpoint.host, endpoint.port))
        }
    }
}

fn windows_proxy_endpoint(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let selected = if value.contains('=') {
        let entries = value
            .split(';')
            .filter_map(|entry| entry.split_once('='))
            .map(|(scheme, endpoint)| (scheme.trim().to_ascii_lowercase(), endpoint.trim()))
            .collect::<Vec<_>>();
        entries
            .iter()
            .find(|(scheme, _)| scheme == "https")
            .or_else(|| entries.iter().find(|(scheme, _)| scheme == "http"))
            .map(|(_, endpoint)| *endpoint)?
    } else {
        value
    };
    normalize_proxy_url(selected)
}

fn enabled_endpoint(enabled: bool, host: &str, port: u16) -> bool {
    enabled && !host.trim().is_empty() && port > 0
}

fn proxy_url(host: &str, port: u16) -> Option<String> {
    normalize_proxy_url(&format!("{}:{port}", host.trim()))
}

fn normalize_proxy_url(value: &str) -> Option<String> {
    let value = value.trim();
    let candidate = if value.starts_with("http://") || value.starts_with("https://") {
        value.to_string()
    } else {
        format!("http://{value}")
    };
    let url = reqwest::Url::parse(&candidate).ok()?;
    matches!(url.scheme(), "http" | "https")
        .then_some(candidate)
        .filter(|_| url.host_str().is_some() && url.port_or_known_default().is_some())
}

async fn create_backup(path: &Path, managed: &ManagedProxyEndpoint) -> Result<(), AppError> {
    let backup = SystemProxyBackup {
        version: SYSTEM_PROXY_BACKUP_VERSION,
        managed: Some(managed.clone()),
        state: capture_system_proxy().await?,
    };
    write_backup_atomic(path, &backup).await
}

async fn restore_and_remove_backup(
    path: &Path,
    backup: &SystemProxyBackup,
    fallback: &ManagedProxyEndpoint,
) -> Result<SystemProxyRestoreOutcome, AppError> {
    let outcome = restore_backup_state(backup, fallback).await?;
    remove_backup(path).await?;
    Ok(outcome)
}

async fn restore_backup_state(
    backup: &SystemProxyBackup,
    fallback: &ManagedProxyEndpoint,
) -> Result<SystemProxyRestoreOutcome, AppError> {
    validate_backup_platform(&backup.state)?;
    let current = capture_system_proxy().await?;
    let managed = managed_endpoint(backup, fallback);
    let external_changes_preserved =
        platform_has_external_changes(&backup.state, &current, &managed)?;
    let plan = build_restore_plan(&backup.state, &current, &managed)?;
    restore_system_proxy(&backup.state, &plan).await?;
    Ok(SystemProxyRestoreOutcome {
        external_changes_preserved,
    })
}

fn managed_endpoint(
    backup: &SystemProxyBackup,
    fallback: &ManagedProxyEndpoint,
) -> ManagedProxyEndpoint {
    backup.managed.clone().unwrap_or_else(|| fallback.clone())
}

fn platform_has_external_changes(
    original: &PlatformProxyState,
    current: &PlatformProxyState,
    managed: &ManagedProxyEndpoint,
) -> Result<bool, AppError> {
    match (original, current) {
        (PlatformProxyState::Windows(original), PlatformProxyState::Windows(current)) => Ok(
            current != original && !windows_proxy_is_owned(current, &managed.host, managed.port),
        ),
        (PlatformProxyState::Macos(original), PlatformProxyState::Macos(current)) => {
            Ok(current != original && !macos_proxy_state_is_owned(current, managed))
        }
        (PlatformProxyState::Linux(original), PlatformProxyState::Linux(current)) => {
            Ok(current != original && !linux_proxy_state_is_owned(current, managed))
        }
        _ => Err(proxy_backup_error(
            "current proxy settings do not match the backup operating system",
        )),
    }
}

fn macos_proxy_state_is_owned(state: &MacosProxyState, managed: &ManagedProxyEndpoint) -> bool {
    !state.services.is_empty()
        && state.services.iter().all(|service| {
            macos_proxy_is_owned(&service.http, &managed.host, managed.port)
                && macos_proxy_is_owned(&service.https, &managed.host, managed.port)
        })
}

fn linux_proxy_state_is_owned(state: &LinuxProxyState, managed: &ManagedProxyEndpoint) -> bool {
    state.mode == "manual"
        && linux_proxy_is_owned(&state.http, &managed.host, managed.port)
        && linux_proxy_is_owned(&state.https, &managed.host, managed.port)
}

fn linux_endpoint_has_external_changes(
    original: &LinuxEndpointProxyState,
    current: &LinuxEndpointProxyState,
    managed: &ManagedProxyEndpoint,
) -> bool {
    value_is_external(&current.host, &managed.host, &original.host)
        || value_is_external(&current.port, &managed.port, &original.port)
}

fn value_is_external<T: PartialEq>(current: &T, managed: &T, original: &T) -> bool {
    current != managed && current != original
}

fn build_restore_plan(
    original: &PlatformProxyState,
    current: &PlatformProxyState,
    managed: &ManagedProxyEndpoint,
) -> Result<PlatformRestorePlan, AppError> {
    match (original, current) {
        (PlatformProxyState::Windows(original), PlatformProxyState::Windows(current)) => {
            let proxy_server = format!("{}:{}", managed.host, managed.port);
            let proxy_server_owned = current.proxy_server.as_deref() == Some(proxy_server.as_str());
            let proxy_server_already_restored = current.proxy_server == original.proxy_server;
            Ok(PlatformRestorePlan::Windows(WindowsRestorePlan {
                proxy_enable: current.proxy_enable == Some(1)
                    && (proxy_server_owned || proxy_server_already_restored),
                proxy_server: proxy_server_owned,
                proxy_override: current.proxy_override.as_deref() == Some("<local>"),
            }))
        }
        (PlatformProxyState::Macos(original), PlatformProxyState::Macos(current)) => {
            let mut services = original
                .services
                .iter()
                .map(|original_service| {
                    let current_service = current
                        .services
                        .iter()
                        .find(|current| current.service == original_service.service);
                    MacosServiceRestorePlan {
                        service: original_service.service.clone(),
                        http: macos_endpoint_restore_plan(
                            Some(&original_service.http),
                            current_service.map(|service| &service.http),
                            managed,
                        ),
                        https: macos_endpoint_restore_plan(
                            Some(&original_service.https),
                            current_service.map(|service| &service.https),
                            managed,
                        ),
                    }
                })
                .collect::<Vec<_>>();
            for current_service in &current.services {
                if original
                    .services
                    .iter()
                    .any(|original| original.service == current_service.service)
                {
                    continue;
                }
                services.push(MacosServiceRestorePlan {
                    service: current_service.service.clone(),
                    http: macos_endpoint_restore_plan(None, Some(&current_service.http), managed),
                    https: macos_endpoint_restore_plan(None, Some(&current_service.https), managed),
                });
            }
            Ok(PlatformRestorePlan::Macos(services))
        }
        (PlatformProxyState::Linux(original), PlatformProxyState::Linux(current)) => {
            let http_external =
                linux_endpoint_has_external_changes(&original.http, &current.http, managed);
            let https_external =
                linux_endpoint_has_external_changes(&original.https, &current.https, managed);
            let endpoints_are_owned_or_restored =
                linux_endpoint_is_owned_or_restored(&current.http, &original.http, managed)
                    && linux_endpoint_is_owned_or_restored(
                        &current.https,
                        &original.https,
                        managed,
                    );
            Ok(PlatformRestorePlan::Linux(LinuxRestorePlan {
                mode: current.mode == "manual" && endpoints_are_owned_or_restored,
                http_host: !http_external && current.http.host == managed.host,
                http_port: !http_external && current.http.port == managed.port,
                https_host: !https_external && current.https.host == managed.host,
                https_port: !https_external && current.https.port == managed.port,
            }))
        }
        _ => Err(proxy_backup_error(
            "current proxy settings do not match the backup operating system",
        )),
    }
}

fn macos_endpoint_restore_plan(
    original: Option<&MacosEndpointProxyState>,
    current: Option<&MacosEndpointProxyState>,
    managed: &ManagedProxyEndpoint,
) -> MacosEndpointRestorePlan {
    let Some(current) = current else {
        return MacosEndpointRestorePlan {
            original: original.cloned(),
            address: false,
            enabled: false,
            current_enabled: false,
        };
    };
    let address_owned = current.server == managed.host && current.port == managed.port;
    let address_already_restored = original
        .is_some_and(|original| current.server == original.server && current.port == original.port);
    MacosEndpointRestorePlan {
        original: original.cloned(),
        address: original.is_some() && address_owned,
        enabled: current.enabled && (address_owned || address_already_restored),
        current_enabled: current.enabled,
    }
}

fn linux_endpoint_is_owned_or_restored(
    current: &LinuxEndpointProxyState,
    original: &LinuxEndpointProxyState,
    managed: &ManagedProxyEndpoint,
) -> bool {
    (current.host == managed.host || current.host == original.host)
        && (current.port == managed.port || current.port == original.port)
}

fn windows_proxy_is_owned(state: &WindowsProxyState, host: &str, port: u16) -> bool {
    let owned_proxy = format!("{host}:{port}");
    state.proxy_enable == Some(1)
        && state.proxy_server.as_deref() == Some(owned_proxy.as_str())
        && state.proxy_override.as_deref() == Some("<local>")
}

fn validate_backup_platform(state: &PlatformProxyState) -> Result<(), AppError> {
    #[cfg(target_os = "windows")]
    let matches = matches!(state, PlatformProxyState::Windows(_));
    #[cfg(target_os = "macos")]
    let matches = matches!(state, PlatformProxyState::Macos(_));
    #[cfg(target_os = "linux")]
    let matches = matches!(state, PlatformProxyState::Linux(_));
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let matches = {
        let _ = state;
        false
    };

    if matches {
        Ok(())
    } else {
        Err(proxy_backup_error(
            "proxy backup belongs to a different operating system",
        ))
    }
}

async fn backup_exists(path: &Path) -> Result<bool, AppError> {
    tokio::fs::try_exists(path)
        .await
        .map_err(|error| proxy_backup_error(format!("failed checking proxy backup: {error}")))
}

async fn read_backup(path: &Path) -> Result<Option<SystemProxyBackup>, AppError> {
    restrict_sensitive_file_permissions(path).map_err(|error| {
        proxy_backup_error(format!("failed securing proxy backup permissions: {error}"))
    })?;
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(proxy_backup_error(format!(
                "failed reading proxy backup: {error}"
            )))
        }
    };
    let backup: SystemProxyBackup = serde_json::from_slice(&bytes)
        .map_err(|error| proxy_backup_error(format!("proxy backup is invalid: {error}")))?;
    if backup.version != SYSTEM_PROXY_BACKUP_VERSION {
        return Err(proxy_backup_error(format!(
            "unsupported proxy backup version {}",
            backup.version
        )));
    }
    Ok(Some(backup))
}

async fn write_backup_atomic(path: &Path, backup: &SystemProxyBackup) -> Result<(), AppError> {
    if backup_exists(path).await? {
        restrict_sensitive_file_permissions(path).map_err(|error| {
            proxy_backup_error(format!("failed securing proxy backup permissions: {error}"))
        })?;
        return Ok(());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        ensure_private_directory(parent).map_err(|error| {
            proxy_backup_error(format!("failed creating proxy backup directory: {error}"))
        })?;
    }
    let bytes = serde_json::to_vec_pretty(backup)
        .map_err(|error| proxy_backup_error(format!("failed encoding proxy backup: {error}")))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("system-proxy-backup.json");
    let staging = path.with_file_name(format!(
        ".{file_name}.{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let result = async {
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(crate::paths::PRIVATE_FILE_MODE);
        let mut file = options.open(&staging).await.map_err(|error| {
            proxy_backup_error(format!("failed creating proxy backup: {error}"))
        })?;
        file.write_all(&bytes)
            .await
            .map_err(|error| proxy_backup_error(format!("failed writing proxy backup: {error}")))?;
        file.sync_all()
            .await
            .map_err(|error| proxy_backup_error(format!("failed syncing proxy backup: {error}")))?;
        drop(file);
        restrict_sensitive_file_permissions(&staging).map_err(|error| {
            proxy_backup_error(format!("failed securing proxy backup permissions: {error}"))
        })?;
        tokio::fs::rename(&staging, path).await.map_err(|error| {
            proxy_backup_error(format!("failed committing proxy backup: {error}"))
        })?;
        restrict_sensitive_file_permissions(path).map_err(|error| {
            proxy_backup_error(format!("failed securing proxy backup permissions: {error}"))
        })
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&staging).await;
    }
    result
}

async fn remove_backup(path: &Path) -> Result<(), AppError> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(proxy_backup_error(format!(
            "failed deleting proxy backup: {error}"
        ))),
    }
}

fn proxy_backup_error(message: impl Into<String>) -> AppError {
    AppError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "system_proxy_backup_failed",
        message,
    )
}

fn platform_state_error(message: impl Into<String>) -> AppError {
    AppError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "platform_command_failed",
        message,
    )
}

pub async fn validate_tun_permissions() -> Result<(), AppError> {
    validate_tun_permissions_inner().await.map_err(|message| {
        AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "tun_permission_required",
            message,
        )
    })
}

#[cfg(target_os = "windows")]
async fn validate_tun_permissions_inner() -> Result<(), String> {
    let script = r#"
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { exit 0 }
exit 1
"#;
    let output = platform_command("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .await
        .map_err(|err| format!("TUN mode requires administrator privileges on Windows: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err("TUN mode requires administrator privileges on Windows. Restart rweb-clash as administrator or disable TUN mode.".into())
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
async fn validate_tun_permissions_inner() -> Result<(), String> {
    use std::os::unix::fs::FileTypeExt;

    let status = tokio::fs::read_to_string("/proc/self/status")
        .await
        .map_err(|err| format!("TUN mode requires CAP_NET_ADMIN on Linux: {err}"))?;
    let cap_eff = status
        .lines()
        .find_map(|line| line.strip_prefix("CapEff:"))
        .map(str::trim)
        .ok_or_else(|| "TUN mode requires CAP_NET_ADMIN on Linux, but current process capabilities could not be read.".to_string())?;
    let caps = u64::from_str_radix(cap_eff, 16)
        .map_err(|err| format!("TUN mode requires CAP_NET_ADMIN on Linux: {err}"))?;
    const CAP_NET_ADMIN: u64 = 1 << 12;
    if caps & CAP_NET_ADMIN == 0 {
        return Err("TUN mode requires CAP_NET_ADMIN on Linux. Start rweb-clash with CAP_NET_ADMIN, run as root, or disable TUN mode.".into());
    }

    const TUN_DEVICE: &str = "/dev/net/tun";
    let metadata = tokio::fs::metadata(TUN_DEVICE)
        .await
        .map_err(|err| format!("TUN mode requires {TUN_DEVICE}: {err}"))?;
    if !metadata.file_type().is_char_device() {
        return Err(format!(
            "TUN mode requires {TUN_DEVICE} to be a character device"
        ));
    }
    tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(TUN_DEVICE)
        .await
        .map_err(|err| format!("TUN mode requires read/write access to {TUN_DEVICE}: {err}"))?;
    Ok(())
}

#[cfg(target_os = "macos")]
async fn validate_tun_permissions_inner() -> Result<(), String> {
    for tool in ["/usr/bin/osascript", "/usr/bin/nc", "/usr/bin/tail"] {
        let metadata = tokio::fs::metadata(tool)
            .await
            .map_err(|error| format!("macOS TUN authorization requires {tool}: {error}"))?;
        if !metadata.is_file() {
            return Err(format!(
                "macOS TUN authorization requires {tool} to be a regular file"
            ));
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "windows", unix)))]
async fn validate_tun_permissions_inner() -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "windows")]
async fn capture_system_proxy() -> Result<PlatformProxyState, AppError> {
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$path = 'Software\Microsoft\Windows\CurrentVersion\Internet Settings'
$key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey($path, $false)
if ($null -eq $key) { throw "Internet Settings registry key was not found" }
function Read-TextValue([string]$name) {
    if ($key.GetValueNames() -notcontains $name) { return $null }
    return [string]$key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
}
function Read-DwordValue([string]$name) {
    if ($key.GetValueNames() -notcontains $name) { return $null }
    return [uint32]$key.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
}
[PSCustomObject]@{
    proxy_enable = Read-DwordValue 'ProxyEnable'
    proxy_server = Read-TextValue 'ProxyServer'
    proxy_override = Read-TextValue 'ProxyOverride'
} | ConvertTo-Json -Compress
"#;
    let output = command_output(
        "powershell",
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            SCRIPT,
        ],
    )
    .await?;
    parse_windows_proxy_state(&output).map(PlatformProxyState::Windows)
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_proxy_state(output: &str) -> Result<WindowsProxyState, AppError> {
    serde_json::from_str(output.trim()).map_err(|error| {
        platform_state_error(format!("failed parsing Windows proxy settings: {error}"))
    })
}

#[cfg(target_os = "windows")]
async fn enable_system_proxy(host: &str, port: u16) -> Result<(), AppError> {
    let proxy = format!("{host}:{port}");
    run_command(
        "reg",
        &[
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyServer",
            "/t",
            "REG_SZ",
            "/d",
            &proxy,
            "/f",
        ],
    )
    .await?;
    run_command(
        "reg",
        &[
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyOverride",
            "/t",
            "REG_SZ",
            "/d",
            "<local>",
            "/f",
        ],
    )
    .await?;
    run_command(
        "reg",
        &[
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyEnable",
            "/t",
            "REG_DWORD",
            "/d",
            "1",
            "/f",
        ],
    )
    .await?;
    notify_windows_proxy_changed().await;
    Ok(())
}

#[cfg(target_os = "windows")]
async fn restore_system_proxy(
    state: &PlatformProxyState,
    plan: &PlatformRestorePlan,
) -> Result<(), AppError> {
    let (PlatformProxyState::Windows(state), PlatformRestorePlan::Windows(plan)) = (state, plan)
    else {
        return Err(proxy_backup_error(
            "proxy backup belongs to a different operating system",
        ));
    };
    let mut changed = false;
    if plan.proxy_server {
        restore_windows_text_value("ProxyServer", state.proxy_server.as_deref()).await?;
        changed = true;
    }
    if plan.proxy_override {
        restore_windows_text_value("ProxyOverride", state.proxy_override.as_deref()).await?;
        changed = true;
    }
    if plan.proxy_enable {
        restore_windows_dword_value("ProxyEnable", state.proxy_enable).await?;
        changed = true;
    }
    if changed {
        notify_windows_proxy_changed().await;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
async fn restore_windows_text_value(name: &str, value: Option<&str>) -> Result<(), AppError> {
    if let Some(value) = value {
        run_command(
            "reg",
            &[
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                "/v",
                name,
                "/t",
                "REG_SZ",
                "/d",
                value,
                "/f",
            ],
        )
        .await
    } else {
        remove_windows_registry_value(name).await
    }
}

#[cfg(target_os = "windows")]
async fn restore_windows_dword_value(name: &str, value: Option<u32>) -> Result<(), AppError> {
    if let Some(value) = value {
        let value = value.to_string();
        run_command(
            "reg",
            &[
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                "/v",
                name,
                "/t",
                "REG_DWORD",
                "/d",
                &value,
                "/f",
            ],
        )
        .await
    } else {
        remove_windows_registry_value(name).await
    }
}

#[cfg(target_os = "windows")]
async fn remove_windows_registry_value(name: &str) -> Result<(), AppError> {
    const SCRIPT: &str = r#"
& {
    param([string]$Name)
    $ErrorActionPreference = 'Stop'
    $path = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
    $key = Get-Item -LiteralPath $path
    if ($key.GetValueNames() -contains $Name) {
        Remove-ItemProperty -LiteralPath $path -Name $Name -ErrorAction Stop
    }
}
"#;
    run_command(
        "powershell",
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            SCRIPT,
            name,
        ],
    )
    .await
}

#[cfg(target_os = "windows")]
async fn clear_owned_system_proxy(host: &str, port: u16) -> Result<(), AppError> {
    let PlatformProxyState::Windows(current) = capture_system_proxy().await? else {
        unreachable!("Windows capture always returns Windows state")
    };
    if !windows_proxy_is_owned(&current, host, port) {
        return Ok(());
    }
    run_command(
        "reg",
        &[
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyEnable",
            "/t",
            "REG_DWORD",
            "/d",
            "0",
            "/f",
        ],
    )
    .await?;
    notify_windows_proxy_changed().await;
    Ok(())
}

#[cfg(target_os = "windows")]
async fn notify_windows_proxy_changed() {
    let script = r#"
Add-Type -Namespace WinInet -Name NativeMethods -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("wininet.dll", SetLastError=true)]
public static extern bool InternetSetOption(System.IntPtr hInternet, int dwOption, System.IntPtr lpBuffer, int dwBufferLength);
'@
[WinInet.NativeMethods]::InternetSetOption([IntPtr]::Zero, 39, [IntPtr]::Zero, 0) | Out-Null
[WinInet.NativeMethods]::InternetSetOption([IntPtr]::Zero, 37, [IntPtr]::Zero, 0) | Out-Null
"#;
    let _ = platform_command("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .await;
}

#[cfg(target_os = "macos")]
async fn capture_system_proxy() -> Result<PlatformProxyState, AppError> {
    let services = list_macos_network_services().await?;
    let mut snapshots = Vec::with_capacity(services.len());
    for service in services {
        let http = read_macos_proxy(&service, false).await?;
        let https = read_macos_proxy(&service, true).await?;
        snapshots.push(MacosServiceProxyState {
            service,
            http,
            https,
        });
    }
    Ok(PlatformProxyState::Macos(MacosProxyState {
        services: snapshots,
    }))
}

#[cfg(any(target_os = "macos", test))]
fn parse_macos_network_services(output: &str) -> Vec<String> {
    output
        .lines()
        .skip(1)
        .map(str::trim)
        .map(|line| line.strip_prefix('*').unwrap_or(line).trim())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(target_os = "macos")]
async fn list_macos_network_services() -> Result<Vec<String>, AppError> {
    let output = command_output("networksetup", &["-listallnetworkservices"]).await?;
    Ok(parse_macos_network_services(&output))
}

#[cfg(target_os = "macos")]
async fn read_macos_proxy(
    service: &str,
    secure: bool,
) -> Result<MacosEndpointProxyState, AppError> {
    let command = if secure {
        "-getsecurewebproxy"
    } else {
        "-getwebproxy"
    };
    let output = command_output("networksetup", &[command, service]).await?;
    parse_macos_proxy_state(&output).map_err(platform_state_error)
}

#[cfg(any(target_os = "macos", test))]
fn parse_macos_proxy_state(output: &str) -> Result<MacosEndpointProxyState, String> {
    let mut enabled = None;
    let mut server = None;
    let mut port = None;
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.trim() {
            "Enabled" => {
                enabled = match value.trim() {
                    value if value.eq_ignore_ascii_case("yes") => Some(true),
                    value if value.eq_ignore_ascii_case("no") => Some(false),
                    value => return Err(format!("unexpected proxy enabled value {value}")),
                }
            }
            "Server" => server = Some(value.trim().to_string()),
            "Port" => {
                port = Some(
                    value
                        .trim()
                        .parse::<u16>()
                        .map_err(|error| format!("invalid proxy port: {error}"))?,
                )
            }
            _ => {}
        }
    }
    Ok(MacosEndpointProxyState {
        enabled: enabled.ok_or_else(|| "proxy output did not include Enabled".to_string())?,
        server: server.ok_or_else(|| "proxy output did not include Server".to_string())?,
        port: port.ok_or_else(|| "proxy output did not include Port".to_string())?,
    })
}

#[cfg(target_os = "macos")]
async fn enable_system_proxy(host: &str, port: u16) -> Result<(), AppError> {
    for service in list_macos_network_services().await? {
        set_macos_proxy_address(&service, false, host, port).await?;
        set_macos_proxy_enabled(&service, false, true).await?;
        set_macos_proxy_address(&service, true, host, port).await?;
        set_macos_proxy_enabled(&service, true, true).await?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn restore_system_proxy(
    state: &PlatformProxyState,
    plan: &PlatformRestorePlan,
) -> Result<(), AppError> {
    let (PlatformProxyState::Macos(_), PlatformRestorePlan::Macos(plan)) = (state, plan) else {
        return Err(proxy_backup_error(
            "proxy backup belongs to a different operating system",
        ));
    };
    for service_plan in plan {
        restore_macos_endpoint(&service_plan.service, false, &service_plan.http).await?;
        restore_macos_endpoint(&service_plan.service, true, &service_plan.https).await?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn restore_macos_endpoint(
    service: &str,
    secure: bool,
    plan: &MacosEndpointRestorePlan,
) -> Result<(), AppError> {
    if plan.address {
        let original = plan
            .original
            .as_ref()
            .expect("address restoration requires an original endpoint");
        set_macos_proxy_address(service, secure, &original.server, original.port).await?;
        if !plan.enabled {
            set_macos_proxy_enabled(service, secure, plan.current_enabled).await?;
        }
    }
    if plan.enabled {
        let enabled = plan
            .original
            .as_ref()
            .is_some_and(|original| original.enabled);
        set_macos_proxy_enabled(service, secure, enabled).await?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn set_macos_proxy_address(
    service: &str,
    secure: bool,
    server: &str,
    port: u16,
) -> Result<(), AppError> {
    let port = port.to_string();
    let command = if secure {
        "-setsecurewebproxy"
    } else {
        "-setwebproxy"
    };
    run_command("networksetup", &[command, service, server, &port]).await
}

#[cfg(target_os = "macos")]
async fn set_macos_proxy_enabled(
    service: &str,
    secure: bool,
    enabled: bool,
) -> Result<(), AppError> {
    let command = if secure {
        "-setsecurewebproxystate"
    } else {
        "-setwebproxystate"
    };
    run_command(
        "networksetup",
        &[command, service, if enabled { "on" } else { "off" }],
    )
    .await
}

#[cfg(target_os = "macos")]
async fn clear_owned_system_proxy(host: &str, port: u16) -> Result<(), AppError> {
    let PlatformProxyState::Macos(current) = capture_system_proxy().await? else {
        unreachable!("macOS capture always returns macOS state")
    };
    for service in current.services {
        if macos_proxy_is_owned(&service.http, host, port) {
            run_command(
                "networksetup",
                &["-setwebproxystate", &service.service, "off"],
            )
            .await?;
        }
        if macos_proxy_is_owned(&service.https, host, port) {
            run_command(
                "networksetup",
                &["-setsecurewebproxystate", &service.service, "off"],
            )
            .await?;
        }
    }
    Ok(())
}

fn macos_proxy_is_owned(state: &MacosEndpointProxyState, host: &str, port: u16) -> bool {
    state.enabled && state.server == host && state.port == port
}

#[cfg(target_os = "linux")]
async fn capture_system_proxy() -> Result<PlatformProxyState, AppError> {
    let mode = read_gsettings_string("org.gnome.system.proxy", "mode").await?;
    let http = read_linux_proxy_endpoint("org.gnome.system.proxy.http").await?;
    let https = read_linux_proxy_endpoint("org.gnome.system.proxy.https").await?;
    Ok(PlatformProxyState::Linux(LinuxProxyState {
        mode,
        http,
        https,
    }))
}

#[cfg(target_os = "linux")]
async fn read_linux_proxy_endpoint(schema: &str) -> Result<LinuxEndpointProxyState, AppError> {
    let host = read_gsettings_string(schema, "host").await?;
    let port = command_output("gsettings", &["get", schema, "port"])
        .await?
        .trim()
        .parse::<u16>()
        .map_err(|error| {
            platform_state_error(format!("failed parsing GNOME proxy port: {error}"))
        })?;
    Ok(LinuxEndpointProxyState { host, port })
}

#[cfg(target_os = "linux")]
async fn read_gsettings_string(schema: &str, key: &str) -> Result<String, AppError> {
    let output = command_output("gsettings", &["get", schema, key]).await?;
    parse_gsettings_string(&output).map_err(platform_state_error)
}

#[cfg(any(target_os = "linux", test))]
fn parse_gsettings_string(output: &str) -> Result<String, String> {
    let value = output.trim();
    if value.len() < 2 || !value.starts_with('\'') || !value.ends_with('\'') {
        return Err(format!("unexpected GSettings string value {value}"));
    }
    let mut decoded = String::new();
    let mut characters = value[1..value.len() - 1].chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            let escaped = characters
                .next()
                .ok_or_else(|| "GSettings string ended with an escape".to_string())?;
            decoded.push(escaped);
        } else {
            decoded.push(character);
        }
    }
    Ok(decoded)
}

#[cfg(any(target_os = "linux", test))]
fn encode_gsettings_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[cfg(target_os = "linux")]
async fn enable_system_proxy(host: &str, port: u16) -> Result<(), AppError> {
    let port = port.to_string();
    let host = encode_gsettings_string(host);
    run_command(
        "gsettings",
        &["set", "org.gnome.system.proxy.http", "host", &host],
    )
    .await?;
    run_command(
        "gsettings",
        &["set", "org.gnome.system.proxy.http", "port", &port],
    )
    .await?;
    run_command(
        "gsettings",
        &["set", "org.gnome.system.proxy.https", "host", &host],
    )
    .await?;
    run_command(
        "gsettings",
        &["set", "org.gnome.system.proxy.https", "port", &port],
    )
    .await?;
    run_command(
        "gsettings",
        &[
            "set",
            "org.gnome.system.proxy",
            "mode",
            &encode_gsettings_string("manual"),
        ],
    )
    .await
}

#[cfg(target_os = "linux")]
async fn restore_system_proxy(
    state: &PlatformProxyState,
    plan: &PlatformRestorePlan,
) -> Result<(), AppError> {
    let (PlatformProxyState::Linux(state), PlatformRestorePlan::Linux(plan)) = (state, plan) else {
        return Err(proxy_backup_error(
            "proxy backup belongs to a different operating system",
        ));
    };
    restore_linux_proxy_endpoint(
        "org.gnome.system.proxy.http",
        &state.http,
        plan.http_host,
        plan.http_port,
    )
    .await?;
    restore_linux_proxy_endpoint(
        "org.gnome.system.proxy.https",
        &state.https,
        plan.https_host,
        plan.https_port,
    )
    .await?;
    if plan.mode {
        let mode = encode_gsettings_string(&state.mode);
        run_command(
            "gsettings",
            &["set", "org.gnome.system.proxy", "mode", &mode],
        )
        .await?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
async fn restore_linux_proxy_endpoint(
    schema: &str,
    state: &LinuxEndpointProxyState,
    restore_host: bool,
    restore_port: bool,
) -> Result<(), AppError> {
    if restore_host {
        let host = encode_gsettings_string(&state.host);
        run_command("gsettings", &["set", schema, "host", &host]).await?;
    }
    if restore_port {
        let port = state.port.to_string();
        run_command("gsettings", &["set", schema, "port", &port]).await?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
async fn clear_owned_system_proxy(host: &str, port: u16) -> Result<(), AppError> {
    let PlatformProxyState::Linux(current) = capture_system_proxy().await? else {
        unreachable!("Linux capture always returns Linux state")
    };
    if current.mode != "manual"
        || !linux_proxy_is_owned(&current.http, host, port)
        || !linux_proxy_is_owned(&current.https, host, port)
    {
        return Ok(());
    }
    let mode = encode_gsettings_string("none");
    run_command(
        "gsettings",
        &["set", "org.gnome.system.proxy", "mode", &mode],
    )
    .await
}

fn linux_proxy_is_owned(state: &LinuxEndpointProxyState, host: &str, port: u16) -> bool {
    state.host == host && state.port == port
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
async fn capture_system_proxy() -> Result<PlatformProxyState, AppError> {
    Err(platform_unsupported())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
async fn enable_system_proxy(_host: &str, _port: u16) -> Result<(), AppError> {
    Err(platform_unsupported())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
async fn restore_system_proxy(
    _state: &PlatformProxyState,
    _plan: &PlatformRestorePlan,
) -> Result<(), AppError> {
    Err(platform_unsupported())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
async fn clear_owned_system_proxy(_host: &str, _port: u16) -> Result<(), AppError> {
    Err(platform_unsupported())
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn platform_unsupported() -> AppError {
    AppError::new(
        StatusCode::NOT_IMPLEMENTED,
        "platform_unsupported",
        "system proxy is not supported on this platform",
    )
}

async fn run_command(program: &str, args: &[&str]) -> Result<(), AppError> {
    command_output(program, args).await.map(|_| ())
}

async fn command_output(program: &str, args: &[&str]) -> Result<String, AppError> {
    let output = platform_command(program)
        .args(args)
        .output()
        .await
        .map_err(|err| {
            AppError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "platform_command_failed",
                format!("failed to execute {program}: {err}"),
            )
        })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if stderr.is_empty() { stdout } else { stderr };
    Err(AppError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "platform_command_failed",
        if message.is_empty() {
            format!("{program} exited with {}", output.status)
        } else {
            message
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn proxy_backup_restricts_existing_and_new_files() {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "rweb-clash-proxy-backup-permissions-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).expect("create backup test directory");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755))
            .expect("set permissive directory mode");
        let path = root.join("system-proxy-backup.json");
        let backup = SystemProxyBackup {
            version: SYSTEM_PROXY_BACKUP_VERSION,
            managed: Some(ManagedProxyEndpoint {
                host: "127.0.0.1".into(),
                port: 7890,
            }),
            state: PlatformProxyState::Linux(LinuxProxyState {
                mode: "none".into(),
                http: LinuxEndpointProxyState {
                    host: String::new(),
                    port: 0,
                },
                https: LinuxEndpointProxyState {
                    host: String::new(),
                    port: 0,
                },
            }),
        };
        std::fs::write(&path, serde_json::to_vec(&backup).unwrap()).expect("write existing backup");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("set permissive backup mode");

        assert_eq!(read_backup(&path).await.unwrap(), Some(backup.clone()));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        std::fs::remove_file(&path).expect("remove existing backup");
        write_backup_atomic(&path, &backup)
            .await
            .expect("write new backup");
        assert_eq!(
            std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        std::fs::remove_dir_all(root).expect("remove backup test directory");
    }

    #[test]
    fn proxy_backup_json_round_trip_preserves_missing_windows_values() {
        let backup = SystemProxyBackup {
            version: SYSTEM_PROXY_BACKUP_VERSION,
            managed: Some(ManagedProxyEndpoint {
                host: "127.0.0.1".into(),
                port: 7890,
            }),
            state: PlatformProxyState::Windows(WindowsProxyState {
                proxy_enable: Some(0),
                proxy_server: None,
                proxy_override: Some("localhost;*.internal".into()),
            }),
        };

        let json = serde_json::to_string(&backup).expect("serialize backup");
        let decoded: SystemProxyBackup = serde_json::from_str(&json).expect("parse backup");

        assert_eq!(decoded, backup);
        assert!(json.contains("\"proxy_server\":null"));
        assert!(json.contains("\"managed\""));
    }

    #[test]
    fn old_proxy_backup_json_uses_the_config_endpoint_as_fallback() {
        let json = r#"{
            "version": 1,
            "platform": "windows",
            "state": {
                "proxy_enable": 0,
                "proxy_server": null,
                "proxy_override": null
            }
        }"#;
        let backup: SystemProxyBackup = serde_json::from_str(json).expect("parse old backup");
        let fallback = ManagedProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 7891,
        };

        assert_eq!(backup.managed, None);
        assert_eq!(managed_endpoint(&backup, &fallback), fallback);
    }

    #[test]
    fn parses_windows_proxy_snapshot_with_absent_registry_values() {
        let parsed = parse_windows_proxy_state(
            r#"{"proxy_enable":1,"proxy_server":"127.0.0.1:7890","proxy_override":null}"#,
        )
        .expect("parse Windows state");

        assert_eq!(parsed.proxy_enable, Some(1));
        assert_eq!(parsed.proxy_server.as_deref(), Some("127.0.0.1:7890"));
        assert_eq!(parsed.proxy_override, None);
    }

    #[test]
    fn windows_proxy_ownership_requires_every_value_written_by_the_app() {
        let owned = WindowsProxyState {
            proxy_enable: Some(1),
            proxy_server: Some("127.0.0.1:7890".into()),
            proxy_override: Some("<local>".into()),
        };
        assert!(windows_proxy_is_owned(&owned, "127.0.0.1", 7890));

        let mut externally_changed = owned;
        externally_changed.proxy_override = Some("*.internal".into());
        assert!(!windows_proxy_is_owned(
            &externally_changed,
            "127.0.0.1",
            7890
        ));
    }

    #[test]
    fn windows_restore_plan_preserves_external_fields_and_restores_owned_fields() {
        let original = PlatformProxyState::Windows(WindowsProxyState {
            proxy_enable: Some(0),
            proxy_server: Some("proxy.before:8080".into()),
            proxy_override: Some("localhost;*.internal".into()),
        });
        let current = PlatformProxyState::Windows(WindowsProxyState {
            proxy_enable: Some(1),
            proxy_server: Some("127.0.0.1:7890".into()),
            proxy_override: Some("external-change".into()),
        });
        let managed = ManagedProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 7890,
        };

        assert_eq!(
            build_restore_plan(&original, &current, &managed).unwrap(),
            PlatformRestorePlan::Windows(WindowsRestorePlan {
                proxy_enable: true,
                proxy_server: true,
                proxy_override: false,
            })
        );
    }

    #[test]
    fn restore_plan_retry_skips_fields_already_restored_after_a_partial_failure() {
        let original = PlatformProxyState::Windows(WindowsProxyState {
            proxy_enable: Some(0),
            proxy_server: Some("proxy.before:8080".into()),
            proxy_override: Some("localhost".into()),
        });
        let current_after_partial_restore = PlatformProxyState::Windows(WindowsProxyState {
            proxy_enable: Some(1),
            proxy_server: Some("proxy.before:8080".into()),
            proxy_override: Some("<local>".into()),
        });
        let managed = ManagedProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 7890,
        };

        assert_eq!(
            build_restore_plan(&original, &current_after_partial_restore, &managed).unwrap(),
            PlatformRestorePlan::Windows(WindowsRestorePlan {
                proxy_enable: true,
                proxy_server: false,
                proxy_override: true,
            })
        );
    }

    #[test]
    fn windows_external_server_prevents_restoring_the_shared_enable_flag() {
        let original = PlatformProxyState::Windows(WindowsProxyState {
            proxy_enable: Some(0),
            proxy_server: Some("proxy.before:8080".into()),
            proxy_override: None,
        });
        let current = PlatformProxyState::Windows(WindowsProxyState {
            proxy_enable: Some(1),
            proxy_server: Some("external-proxy:9000".into()),
            proxy_override: Some("<local>".into()),
        });
        let managed = ManagedProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 7890,
        };

        let PlatformRestorePlan::Windows(plan) =
            build_restore_plan(&original, &current, &managed).unwrap()
        else {
            panic!("expected Windows restore plan");
        };
        assert!(!plan.proxy_enable);
        assert!(!plan.proxy_server);
        assert!(plan.proxy_override);
        assert!(platform_has_external_changes(&original, &current, &managed).unwrap());
    }

    #[test]
    fn clean_windows_crash_has_no_external_takeover() {
        let original = PlatformProxyState::Windows(WindowsProxyState {
            proxy_enable: Some(0),
            proxy_server: Some("proxy.before:8080".into()),
            proxy_override: None,
        });
        let current = PlatformProxyState::Windows(WindowsProxyState {
            proxy_enable: Some(1),
            proxy_server: Some("127.0.0.1:7890".into()),
            proxy_override: Some("<local>".into()),
        });
        let managed = ManagedProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 7890,
        };

        assert!(!platform_has_external_changes(&original, &current, &managed).unwrap());
        assert!(!platform_has_external_changes(&original, &original, &managed).unwrap());
    }

    #[test]
    fn parses_all_macos_network_services_including_disabled_services() {
        let output =
            "An asterisk (*) denotes that a network service is disabled.\nWi-Fi\n*Bluetooth PAN\n";

        assert_eq!(
            parse_macos_network_services(output),
            vec!["Wi-Fi".to_string(), "Bluetooth PAN".to_string()]
        );
    }

    #[test]
    fn parses_macos_proxy_endpoint_state() {
        let parsed = parse_macos_proxy_state(
            "Enabled: Yes\nServer: proxy.example\nPort: 8443\nAuthenticated Proxy Enabled: 0\n",
        )
        .expect("parse macOS state");

        assert_eq!(
            parsed,
            MacosEndpointProxyState {
                enabled: true,
                server: "proxy.example".into(),
                port: 8443,
            }
        );
        assert!(macos_proxy_is_owned(
            &MacosEndpointProxyState {
                enabled: true,
                server: "127.0.0.1".into(),
                port: 7890,
            },
            "127.0.0.1",
            7890
        ));
    }

    #[test]
    fn macos_restore_plan_is_independent_per_service_and_protocol() {
        let original = PlatformProxyState::Macos(MacosProxyState {
            services: vec![MacosServiceProxyState {
                service: "Wi-Fi".into(),
                http: MacosEndpointProxyState {
                    enabled: false,
                    server: "before-http".into(),
                    port: 8080,
                },
                https: MacosEndpointProxyState {
                    enabled: false,
                    server: "before-https".into(),
                    port: 8443,
                },
            }],
        });
        let current = PlatformProxyState::Macos(MacosProxyState {
            services: vec![MacosServiceProxyState {
                service: "Wi-Fi".into(),
                http: MacosEndpointProxyState {
                    enabled: true,
                    server: "external-http".into(),
                    port: 9000,
                },
                https: MacosEndpointProxyState {
                    enabled: true,
                    server: "127.0.0.1".into(),
                    port: 7890,
                },
            }],
        });
        let managed = ManagedProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 7890,
        };

        assert_eq!(
            build_restore_plan(&original, &current, &managed).unwrap(),
            PlatformRestorePlan::Macos(vec![MacosServiceRestorePlan {
                service: "Wi-Fi".into(),
                http: MacosEndpointRestorePlan {
                    original: Some(MacosEndpointProxyState {
                        enabled: false,
                        server: "before-http".into(),
                        port: 8080,
                    }),
                    address: false,
                    enabled: false,
                    current_enabled: true,
                },
                https: MacosEndpointRestorePlan {
                    original: Some(MacosEndpointProxyState {
                        enabled: false,
                        server: "before-https".into(),
                        port: 8443,
                    }),
                    address: true,
                    enabled: true,
                    current_enabled: true,
                },
            }])
        );
        assert!(platform_has_external_changes(&original, &current, &managed).unwrap());
    }

    #[test]
    fn macos_disabled_managed_endpoint_is_external_takeover() {
        let endpoint = MacosEndpointProxyState {
            enabled: true,
            server: "127.0.0.1".into(),
            port: 7890,
        };
        let original = PlatformProxyState::Macos(MacosProxyState {
            services: vec![MacosServiceProxyState {
                service: "Wi-Fi".into(),
                http: MacosEndpointProxyState {
                    enabled: false,
                    server: "before-http".into(),
                    port: 8080,
                },
                https: MacosEndpointProxyState {
                    enabled: false,
                    server: "before-https".into(),
                    port: 8443,
                },
            }],
        });
        let mut disabled_http = endpoint.clone();
        disabled_http.enabled = false;
        let current = PlatformProxyState::Macos(MacosProxyState {
            services: vec![MacosServiceProxyState {
                service: "Wi-Fi".into(),
                http: disabled_http,
                https: endpoint,
            }],
        });
        let managed = ManagedProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 7890,
        };

        assert!(platform_has_external_changes(&original, &current, &managed).unwrap());
    }

    #[test]
    fn macos_new_non_managed_service_is_external_takeover() {
        let original = PlatformProxyState::Macos(MacosProxyState { services: vec![] });
        let current = PlatformProxyState::Macos(MacosProxyState {
            services: vec![MacosServiceProxyState {
                service: "VPN".into(),
                http: MacosEndpointProxyState {
                    enabled: true,
                    server: "external-http".into(),
                    port: 8080,
                },
                https: MacosEndpointProxyState {
                    enabled: true,
                    server: "external-https".into(),
                    port: 8443,
                },
            }],
        });
        let managed = ManagedProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 7890,
        };

        assert!(platform_has_external_changes(&original, &current, &managed).unwrap());
    }

    #[test]
    fn macos_new_managed_service_is_disabled_without_overwriting_its_address() {
        let original = PlatformProxyState::Macos(MacosProxyState {
            services: Vec::new(),
        });
        let managed_endpoint = MacosEndpointProxyState {
            enabled: true,
            server: "127.0.0.1".into(),
            port: 7890,
        };
        let current = PlatformProxyState::Macos(MacosProxyState {
            services: vec![MacosServiceProxyState {
                service: "USB LAN".into(),
                http: managed_endpoint.clone(),
                https: managed_endpoint,
            }],
        });
        let managed = ManagedProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 7890,
        };

        let PlatformRestorePlan::Macos(plan) =
            build_restore_plan(&original, &current, &managed).unwrap()
        else {
            panic!("expected macOS restore plan");
        };
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].service, "USB LAN");
        assert!(plan[0].http.enabled);
        assert!(!plan[0].http.address);
        assert_eq!(plan[0].http.original, None);
        assert!(plan[0].https.enabled);
        assert!(!plan[0].https.address);
    }

    #[test]
    fn gsettings_strings_round_trip_escaped_characters() {
        let original = r"proxy\host's endpoint";
        let encoded = encode_gsettings_string(original);

        assert_eq!(parse_gsettings_string(&encoded).unwrap(), original);
    }

    #[test]
    fn linux_owned_proxy_requires_matching_host_and_port() {
        let state = LinuxEndpointProxyState {
            host: "127.0.0.1".into(),
            port: 7890,
        };

        assert!(linux_proxy_is_owned(&state, "127.0.0.1", 7890));
        assert!(!linux_proxy_is_owned(&state, "127.0.0.1", 7891));
        assert!(!linux_proxy_is_owned(&state, "proxy.example", 7890));
    }

    #[test]
    fn linux_restore_plan_handles_partial_commands_and_external_takeover() {
        let original = PlatformProxyState::Linux(LinuxProxyState {
            mode: "none".into(),
            http: LinuxEndpointProxyState {
                host: "before-http".into(),
                port: 8080,
            },
            https: LinuxEndpointProxyState {
                host: "before-https".into(),
                port: 8443,
            },
        });
        let current = PlatformProxyState::Linux(LinuxProxyState {
            mode: "manual".into(),
            http: LinuxEndpointProxyState {
                host: "external-http".into(),
                port: 7890,
            },
            https: LinuxEndpointProxyState {
                host: "127.0.0.1".into(),
                port: 7890,
            },
        });
        let managed = ManagedProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 7890,
        };

        assert_eq!(
            build_restore_plan(&original, &current, &managed).unwrap(),
            PlatformRestorePlan::Linux(LinuxRestorePlan {
                mode: false,
                http_host: false,
                http_port: false,
                https_host: true,
                https_port: true,
            })
        );
        assert!(platform_has_external_changes(&original, &current, &managed).unwrap());
    }

    #[test]
    fn linux_partial_restore_is_retryable_but_disables_persisted_intent() {
        let original_endpoint = LinuxEndpointProxyState {
            host: "before".into(),
            port: 8080,
        };
        let original = PlatformProxyState::Linux(LinuxProxyState {
            mode: "none".into(),
            http: original_endpoint.clone(),
            https: original_endpoint.clone(),
        });
        let current = PlatformProxyState::Linux(LinuxProxyState {
            mode: "manual".into(),
            http: LinuxEndpointProxyState {
                host: original_endpoint.host.clone(),
                port: 7890,
            },
            https: LinuxEndpointProxyState {
                host: "127.0.0.1".into(),
                port: original_endpoint.port,
            },
        });
        let managed = ManagedProxyEndpoint {
            host: "127.0.0.1".into(),
            port: 7890,
        };

        assert_eq!(
            build_restore_plan(&original, &current, &managed).unwrap(),
            PlatformRestorePlan::Linux(LinuxRestorePlan {
                mode: true,
                http_host: false,
                http_port: true,
                https_host: true,
                https_port: false,
            })
        );
        assert!(platform_has_external_changes(&original, &current, &managed).unwrap());
    }
}
