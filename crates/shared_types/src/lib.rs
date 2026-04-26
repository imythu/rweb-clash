use serde::{Deserialize, Serialize};

pub const MERGED_PROFILE_ID: &str = "__merged_all__";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResponse {
    pub success: bool,
    pub message: String,
}

impl OperationResponse {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfoResponse {
    pub platform: String,
    pub app_dir: String,
    pub runtime_config: String,
    pub api_addr: String,
    pub controller_addr: String,
    pub mihomo_expected_path: String,
    pub mihomo_path: Option<String>,
    pub active_profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreRunState {
    NotRunning,
    Starting,
    Running,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreStatusResponse {
    pub state: CoreRunState,
    pub pid: Option<u32>,
    pub active_profile_id: Option<String>,
    pub started_at: Option<String>,
    pub last_error: Option<String>,
    pub controller_addr: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MihomoInstalledVersion {
    pub tag: String,
    pub asset_name: String,
    pub binary_path: String,
    pub downloaded_at: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClashMode {
    Rule,
    Global,
    Direct,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClashLogLevel {
    Silent,
    Error,
    Warning,
    Info,
    Debug,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClashBasicConfig {
    pub mixed_port: u16,
    pub port: Option<u16>,
    pub socks_port: Option<u16>,
    pub allow_lan: bool,
    pub mode: ClashMode,
    pub log_level: ClashLogLevel,
    pub ipv6: bool,
    pub external_controller: String,
    pub secret: String,
}

impl Default for ClashBasicConfig {
    fn default() -> Self {
        Self {
            mixed_port: 7890,
            port: None,
            socks_port: None,
            allow_lan: false,
            mode: ClashMode::Rule,
            log_level: ClashLogLevel::Info,
            ipv6: false,
            external_controller: "127.0.0.1:9090".into(),
            secret: "rweb-clash".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSystemConfigRequest {
    pub clash: ClashBasicConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfigResponse {
    pub clash: ClashBasicConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    Remote,
    Local,
    Merged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileSourceSummary {
    Url {
        url: String,
        #[serde(default)]
        response_name: Option<String>,
        #[serde(default)]
        request_headers: Vec<HttpHeaderEntry>,
    },
    File {
        filename: Option<String>,
    },
    Merged {
        description: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpHeaderEntry {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub kind: ProfileKind,
    pub source: ProfileSourceSummary,
    pub active: bool,
    pub has_custom_name: bool,
    pub upload: Option<u64>,
    pub download: Option<u64>,
    pub total: Option<u64>,
    pub expire: Option<u64>,
    pub script_id: Option<String>,
    pub script_name: Option<String>,
    pub refresh_interval_hours: u8,
    pub last_refreshed_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptSummary {
    pub id: String,
    pub name: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptDetailResponse {
    pub id: String,
    pub name: String,
    pub script_code: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileDetailResponse {
    pub id: String,
    pub name: String,
    pub kind: ProfileKind,
    pub source: ProfileSourceSummary,
    pub active: bool,
    pub has_custom_name: bool,
    pub upload: Option<u64>,
    pub download: Option<u64>,
    pub total: Option<u64>,
    pub expire: Option<u64>,
    pub script_id: Option<String>,
    pub script_name: Option<String>,
    pub refresh_interval_hours: u8,
    pub last_refreshed_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilePreviewResponse {
    pub profile_id: String,
    pub profile_name: String,
    pub source_summary: String,
    pub has_custom_name: bool,
    pub upload: Option<u64>,
    pub download: Option<u64>,
    pub total: Option<u64>,
    pub expire: Option<u64>,
    pub script_name: Option<String>,
    pub refresh_interval_hours: u8,
    pub raw_content: Option<String>,
    pub rendered_content: Option<String>,
    pub root_kind: Option<String>,
    pub validation_error: Option<String>,
    pub is_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProfileRequest {
    pub name: Option<String>,
    pub url: Option<String>,
    pub request_headers: Option<Vec<HttpHeaderEntry>>,
    pub script_id: Option<String>,
    pub refresh_interval_hours: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportUrlRequest {
    pub name: String,
    pub url: String,
    pub request_headers: Option<Vec<HttpHeaderEntry>>,
    pub script_id: Option<String>,
    pub refresh_interval_hours: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportFileRequest {
    pub name: String,
    pub filename: Option<String>,
    pub content: String,
    pub script_id: Option<String>,
    pub refresh_interval_hours: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertScriptRequest {
    pub name: String,
    pub script_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateScriptRequest {
    pub name: Option<String>,
    pub script_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectProxyRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyDelayResponse {
    pub name: String,
    pub delay: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyHistoryItem {
    pub time: Option<String>,
    pub delay: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyGroupSummary {
    pub name: String,
    pub kind: Option<String>,
    pub now: Option<String>,
    pub all: Vec<String>,
    pub history: Vec<ProxyHistoryItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionSummary {
    pub id: String,
    pub host: Option<String>,
    pub destination_ip: Option<String>,
    pub destination_port: Option<String>,
    pub network: Option<String>,
    pub r#type: Option<String>,
    pub process: Option<String>,
    pub rule: Option<String>,
    pub rule_payload: Option<String>,
    pub chains: Vec<String>,
    pub upload: u64,
    pub download: u64,
    pub start: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts: String,
    pub level: String,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum ServerEvent {
    Log(LogEntry),
    CoreStatus(CoreStatusResponse),
    Profiles(Vec<ProfileSummary>),
}

impl ServerEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::Log(_) => "log",
            Self::CoreStatus(_) => "core_status",
            Self::Profiles(_) => "profiles",
        }
    }
}
