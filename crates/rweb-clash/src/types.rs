use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const SUB_DELIMITER: &str = "^_^";
pub const BUILTIN_DIRECT: &str = "DIRECT";
pub const BUILTIN_REJECT: &str = "REJECT";
pub const BUILTIN_GLOBAL: &str = "GLOBAL";
pub const BUILTIN_PROXY: &str = "PROXY";
pub const DEFAULT_DELAY_TEST_URL: &str = "https://cp.cloudflare.com/generate_204";
pub const DEFAULT_DELAY_TIMEOUT_MS: u64 = 10_000;
pub const MIN_REMOTE_REFRESH_INTERVAL_SECONDS: u64 = 21_600;
pub const DEFAULT_SUBSCRIPTION_REFRESH_INTERVAL_SECONDS: u64 = 21_600;
pub const DEFAULT_RULE_SET_REFRESH_INTERVAL_SECONDS: u64 = 86_400;
pub const MIN_ACTIVE_PROBE_INTERVAL_SECONDS: i64 = 1_800;
pub const DEFAULT_ACTIVE_PROBE_INTERVAL_SECONDS: i64 = 1_800;
pub const MAX_ACTIVE_PROBE_INTERVAL_SECONDS: i64 = 86_400;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemConfig {
    pub allow_lan: bool,
    pub ipv6: bool,
    pub log_level: String,
    pub mixed_port: u16,
    pub external_controller: String,
    pub external_controller_enabled: bool,
    pub secret: String,
    pub dns_enabled: bool,
    pub dns_mode: String,
    pub dns_nameservers: Vec<String>,
    pub dns_fallback: Vec<String>,
    pub dns_fake_ip_filter: Vec<String>,
    pub dns_nameserver_policy: BTreeMap<String, Vec<String>>,
    pub dns_hosts: BTreeMap<String, Vec<String>>,
    pub store_selected: bool,
    pub unified_delay: bool,
    pub tcp_concurrent: bool,
    pub delay_test_url: String,
    pub delay_test_timeout_ms: u64,
    pub tun: bool,
    pub system_proxy: bool,
    pub mode: String,
    pub auto_start: bool,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            allow_lan: false,
            ipv6: true,
            log_level: "info".into(),
            mixed_port: 7890,
            external_controller: "127.0.0.1:9090".into(),
            external_controller_enabled: true,
            secret: "r-clash-secret".into(),
            dns_enabled: true,
            dns_mode: "fake-ip".into(),
            dns_nameservers: vec![
                "https://dns.alidns.com/dns-query".into(),
                "https://doh.pub/dns-query".into(),
            ],
            dns_fallback: Vec::new(),
            dns_fake_ip_filter: Vec::new(),
            dns_nameserver_policy: BTreeMap::new(),
            dns_hosts: BTreeMap::new(),
            store_selected: true,
            unified_delay: true,
            tcp_concurrent: false,
            delay_test_url: DEFAULT_DELAY_TEST_URL.into(),
            delay_test_timeout_ms: DEFAULT_DELAY_TIMEOUT_MS,
            tun: false,
            system_proxy: false,
            mode: "rule".into(),
            auto_start: false,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SystemConfigPatch {
    pub allow_lan: Option<bool>,
    pub ipv6: Option<bool>,
    pub log_level: Option<String>,
    pub mixed_port: Option<u16>,
    pub external_controller: Option<String>,
    pub external_controller_enabled: Option<bool>,
    pub secret: Option<String>,
    pub dns_enabled: Option<bool>,
    pub dns_mode: Option<String>,
    pub dns_nameservers: Option<Vec<String>>,
    pub dns_fallback: Option<Vec<String>>,
    pub dns_fake_ip_filter: Option<Vec<String>>,
    pub dns_nameserver_policy: Option<BTreeMap<String, Vec<String>>>,
    pub dns_hosts: Option<BTreeMap<String, Vec<String>>>,
    pub store_selected: Option<bool>,
    pub unified_delay: Option<bool>,
    pub tcp_concurrent: Option<bool>,
    pub delay_test_url: Option<String>,
    pub delay_test_timeout_ms: Option<u64>,
    pub tun: Option<bool>,
    pub system_proxy: Option<bool>,
    pub mode: Option<String>,
    pub auto_start: Option<bool>,
}

impl SystemConfigPatch {
    pub fn apply(self, config: &mut SystemConfig) {
        if let Some(value) = self.allow_lan {
            config.allow_lan = value;
        }
        if let Some(value) = self.ipv6 {
            config.ipv6 = value;
        }
        if let Some(value) = self.log_level {
            config.log_level = value;
        }
        if let Some(value) = self.mixed_port {
            config.mixed_port = value;
        }
        if let Some(value) = self.external_controller {
            config.external_controller = value;
        }
        if let Some(value) = self.external_controller_enabled {
            config.external_controller_enabled = value;
        }
        if let Some(value) = self.secret {
            config.secret = value;
        }
        if let Some(value) = self.dns_enabled {
            config.dns_enabled = value;
        }
        if let Some(value) = self.dns_mode {
            config.dns_mode = value;
        }
        if let Some(value) = self.dns_nameservers {
            config.dns_nameservers = value;
        }
        if let Some(value) = self.dns_fallback {
            config.dns_fallback = value;
        }
        if let Some(value) = self.dns_fake_ip_filter {
            config.dns_fake_ip_filter = value;
        }
        if let Some(value) = self.dns_nameserver_policy {
            config.dns_nameserver_policy = value;
        }
        if let Some(value) = self.dns_hosts {
            config.dns_hosts = value;
        }
        if let Some(value) = self.store_selected {
            config.store_selected = value;
        }
        if let Some(value) = self.unified_delay {
            config.unified_delay = value;
        }
        if let Some(value) = self.tcp_concurrent {
            config.tcp_concurrent = value;
        }
        if let Some(value) = self.delay_test_url {
            config.delay_test_url = value;
        }
        if let Some(value) = self.delay_test_timeout_ms {
            config.delay_test_timeout_ms = value;
        }
        if let Some(value) = self.tun {
            config.tun = value;
        }
        if let Some(value) = self.system_proxy {
            config.system_proxy = value;
        }
        if let Some(value) = self.mode {
            config.mode = value;
        }
        if let Some(value) = self.auto_start {
            config.auto_start = value;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DownloadRoute {
    Direct,
    Core,
    System,
    #[default]
    Auto,
}

impl DownloadRoute {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Core => "core",
            Self::System => "system",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterRule {
    pub id: String,
    pub action: String,
    #[serde(rename = "type")]
    pub match_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pattern: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct FilterRuleInput {
    pub id: Option<String>,
    pub action: String,
    #[serde(rename = "type", alias = "match_type", alias = "matchType")]
    pub match_type: String,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub values: Vec<String>,
    pub enabled: Option<bool>,
}

impl Default for FilterRuleInput {
    fn default() -> Self {
        Self {
            id: None,
            action: "discard".into(),
            match_type: "contains".into(),
            pattern: String::new(),
            values: Vec::new(),
            enabled: Some(true),
        }
    }
}

impl FilterRule {
    pub fn has_values(&self) -> bool {
        self.values.iter().any(|value| !value.trim().is_empty())
    }

    pub fn effective_values(&self) -> Vec<&str> {
        if self.has_values() {
            self.values
                .iter()
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect()
        } else {
            self.pattern
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect()
        }
    }
}

impl FilterRuleInput {
    pub fn has_values(&self) -> bool {
        self.values.iter().any(|value| !value.trim().is_empty())
    }

    pub fn is_pattern_empty(&self) -> bool {
        self.pattern.trim().is_empty() && !self.has_values()
    }

    pub fn effective_values(&self) -> Vec<&str> {
        if self.has_values() {
            self.values
                .iter()
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect()
        } else {
            self.pattern
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect()
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SubscriptionInput {
    pub name: String,
    pub url: String,
    #[serde(alias = "intervalSeconds")]
    pub interval_seconds: Option<u64>,
    pub interval: Option<u64>,
    #[serde(alias = "inheritGlobal")]
    pub inherit_global: Option<bool>,
    pub rules: Vec<FilterRuleInput>,
    #[serde(alias = "downloadRoute")]
    pub download_route: DownloadRoute,
}

impl Default for SubscriptionInput {
    fn default() -> Self {
        Self {
            name: String::new(),
            url: String::new(),
            interval_seconds: None,
            interval: None,
            inherit_global: Some(true),
            rules: Vec::new(),
            download_route: DownloadRoute::Auto,
        }
    }
}

impl SubscriptionInput {
    pub fn interval_seconds(&self) -> u64 {
        normalize_remote_refresh_interval(
            self.interval_seconds
                .or_else(|| self.interval.map(|minutes| minutes.saturating_mul(60)))
                .unwrap_or(DEFAULT_SUBSCRIPTION_REFRESH_INTERVAL_SECONDS),
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionResponse {
    pub id: String,
    pub name: String,
    pub url: String,
    pub format: String,
    pub nodes: i64,
    pub status: String,
    pub traffic: TrafficQuota,
    pub expiry: Option<String>,
    #[serde(rename = "intervalSeconds")]
    pub interval_seconds: i64,
    pub interval: i64,
    #[serde(rename = "inheritGlobal")]
    pub inherit_global: bool,
    pub rules: Vec<FilterRule>,
    pub breakdown: serde_json::Map<String, Value>,
    #[serde(rename = "lastUpdate")]
    pub last_update: Option<String>,
    #[serde(rename = "lastError")]
    pub last_error: Option<String>,
    #[serde(rename = "downloadRoute")]
    pub download_route: DownloadRoute,
    #[serde(rename = "lastRoute")]
    pub last_route: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionMembersResponse {
    #[serde(rename = "subscriptionId")]
    pub subscription_id: String,
    #[serde(rename = "subscriptionName")]
    pub subscription_name: String,
    pub filtered: SubscriptionMemberSection,
    #[serde(rename = "beforeFilter")]
    pub before_filter: SubscriptionMemberSection,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SubscriptionMemberSection {
    pub nodes: Vec<SubscriptionMemberNode>,
    pub groups: Vec<SubscriptionMemberGroup>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionMemberNode {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub protocol: String,
    pub country: Option<String>,
    pub latency: i64,
    #[serde(rename = "filteredOut")]
    pub filtered_out: bool,
    #[serde(rename = "filterReason")]
    pub filter_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionMemberGroup {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "type")]
    pub group_type: String,
    pub members: Vec<String>,
    #[serde(rename = "memberCount")]
    pub member_count: usize,
    #[serde(rename = "filteredOut")]
    pub filtered_out: bool,
    #[serde(rename = "filterReason")]
    pub filter_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TrafficQuota {
    pub used: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ProxyGroupRequest {
    pub name: String,
    #[serde(rename = "type", alias = "groupType")]
    pub group_type: String,
    pub filter: Vec<GroupFilterInput>,
}

impl Default for ProxyGroupRequest {
    fn default() -> Self {
        Self {
            name: String::new(),
            group_type: "select".into(),
            filter: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct GroupFilterInput {
    pub id: Option<String>,
    pub action: String,
    #[serde(rename = "type", alias = "field")]
    pub field: String,
    pub operator: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub value: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    pub enabled: Option<bool>,
}

impl Default for GroupFilterInput {
    fn default() -> Self {
        Self {
            id: None,
            action: "keep".into(),
            field: "name".into(),
            operator: "contains".into(),
            value: String::new(),
            values: Vec::new(),
            enabled: Some(true),
        }
    }
}

impl GroupFilterInput {
    pub fn has_values(&self) -> bool {
        self.values.iter().any(|value| !value.trim().is_empty())
    }

    pub fn is_value_empty(&self) -> bool {
        self.value.trim().is_empty() && !self.has_values()
    }

    pub fn effective_values(&self) -> Vec<&str> {
        if self.has_values() {
            self.values
                .iter()
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect()
        } else {
            self.value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .collect()
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProxyTopologyResponse {
    pub groups: Vec<ProxyGroupResponse>,
    pub nodes: Vec<ProxyNodeResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProxyGroupResponse {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "type")]
    pub group_type: String,
    pub source: String,
    #[serde(rename = "subscriptionName")]
    pub subscription_name: Option<String>,
    pub builtin: bool,
    pub now: Option<String>,
    pub delay: i64,
    pub all: Vec<String>,
    pub filter: Vec<GroupFilterInput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProxyNodeResponse {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "type")]
    pub protocol: String,
    pub latency: i64,
    pub country: Option<String>,
    #[serde(rename = "subscriptionId")]
    pub subscription_id: Option<String>,
    #[serde(rename = "subscriptionName")]
    pub subscription_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelectProxyRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DelayResponse {
    pub name: String,
    pub delay: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RuleInput {
    pub id: Option<String>,
    #[serde(rename = "type", alias = "rule_type", alias = "ruleType")]
    pub rule_type: String,
    pub value: String,
    pub policy: String,
    pub desc: Option<String>,
    pub enabled: Option<bool>,
    /// One-based position within rules from the same source.
    pub position: Option<usize>,
}

impl Default for RuleInput {
    fn default() -> Self {
        Self {
            id: None,
            rule_type: "DOMAIN-SUFFIX".into(),
            value: String::new(),
            policy: BUILTIN_DIRECT.into(),
            desc: None,
            enabled: Some(true),
            position: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub rule_type: String,
    pub value: String,
    pub policy: String,
    pub position: i64,
    pub source: String,
    pub enabled: bool,
    pub desc: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RuleSetInput {
    pub name: String,
    pub url: String,
    #[serde(alias = "intervalSeconds")]
    pub interval_seconds: Option<u64>,
    pub interval: Option<u64>,
    pub behavior: Option<String>,
    pub format: Option<String>,
    #[serde(alias = "downloadRoute")]
    pub download_route: DownloadRoute,
}

impl Default for RuleSetInput {
    fn default() -> Self {
        Self {
            name: String::new(),
            url: String::new(),
            interval_seconds: None,
            interval: None,
            behavior: None,
            format: Some("text".into()),
            download_route: DownloadRoute::Auto,
        }
    }
}

impl RuleSetInput {
    pub fn interval_seconds(&self) -> u64 {
        normalize_remote_refresh_interval(
            self.interval_seconds
                .or(self.interval)
                .unwrap_or(DEFAULT_RULE_SET_REFRESH_INTERVAL_SECONDS),
        )
    }
}

fn normalize_remote_refresh_interval(interval_seconds: u64) -> u64 {
    if interval_seconds == 0 {
        0
    } else {
        interval_seconds.max(MIN_REMOTE_REFRESH_INTERVAL_SECONDS)
    }
}

#[cfg(test)]
mod interval_tests {
    use super::*;

    #[test]
    fn automatic_remote_refreshes_keep_a_six_hour_floor() {
        let subscription = SubscriptionInput {
            interval_seconds: Some(60),
            ..SubscriptionInput::default()
        };
        assert_eq!(
            subscription.interval_seconds(),
            MIN_REMOTE_REFRESH_INTERVAL_SECONDS
        );

        let legacy_subscription = SubscriptionInput {
            interval: Some(1),
            ..SubscriptionInput::default()
        };
        assert_eq!(
            legacy_subscription.interval_seconds(),
            MIN_REMOTE_REFRESH_INTERVAL_SECONDS
        );

        let rule_set = RuleSetInput {
            interval_seconds: Some(300),
            ..RuleSetInput::default()
        };
        assert_eq!(
            rule_set.interval_seconds(),
            MIN_REMOTE_REFRESH_INTERVAL_SECONDS
        );
    }

    #[test]
    fn zero_still_disables_automatic_remote_refreshes() {
        let subscription = SubscriptionInput {
            interval_seconds: Some(0),
            ..SubscriptionInput::default()
        };
        let rule_set = RuleSetInput {
            interval_seconds: Some(0),
            ..RuleSetInput::default()
        };
        assert_eq!(subscription.interval_seconds(), 0);
        assert_eq!(rule_set.interval_seconds(), 0);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleSetResponse {
    pub id: String,
    pub name: String,
    pub url: String,
    pub behavior: Option<String>,
    pub format: String,
    #[serde(rename = "ruleCount")]
    pub rule_count: i64,
    #[serde(rename = "lastUpdate")]
    pub last_update: Option<String>,
    #[serde(rename = "lastError")]
    pub last_error: Option<String>,
    #[serde(rename = "downloadRoute")]
    pub download_route: DownloadRoute,
    #[serde(rename = "lastRoute")]
    pub last_route: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleTestRequest {
    pub target: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleTestResponse {
    #[serde(rename = "hitRule")]
    pub hit_rule: RuleResponse,
    #[serde(rename = "finalProxy")]
    pub final_proxy: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntryResponse {
    pub time: String,
    pub level: String,
    pub payload: String,
    #[serde(rename = "parsedHost")]
    pub parsed_host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficResponse {
    pub up: u64,
    pub down: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionResponse {
    pub id: String,
    pub domain: Option<String>,
    pub rule: Option<String>,
    pub policy: Option<String>,
    pub speed: String,
    pub network: Option<String>,
    #[serde(rename = "type")]
    pub connection_type: Option<String>,
    #[serde(rename = "sourceIp")]
    pub source_ip: Option<String>,
    #[serde(rename = "sourcePort")]
    pub source_port: Option<String>,
    #[serde(rename = "destinationIp")]
    pub destination_ip: Option<String>,
    #[serde(rename = "destinationPort")]
    pub destination_port: Option<String>,
    pub process: Option<String>,
    pub start: Option<String>,
    pub upload: u64,
    pub download: u64,
    pub chains: Vec<String>,
    #[serde(rename = "rulePayload")]
    pub rule_payload: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionHistoryResponse {
    pub protocol: String,
    pub domain: Option<String>,
    #[serde(rename = "destinationIp")]
    pub destination_ip: Option<String>,
    pub port: String,
    pub network: Option<String>,
    pub policy: Option<String>,
    pub process: Option<String>,
    pub rule: Option<String>,
    #[serde(rename = "firstSeenAt")]
    pub first_seen_at: String,
    #[serde(rename = "lastSeenAt")]
    pub last_seen_at: String,
    #[serde(rename = "seenCount")]
    pub seen_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManualNodeInput {
    pub name: String,
    pub config: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualNodeResponse {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "type")]
    pub protocol: String,
    pub config: Value,
    pub latency: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WebDavSettingsInput {
    pub endpoint: String,
    pub username: String,
    pub password: Option<String>,
    #[serde(alias = "remotePath")]
    pub remote_path: String,
    pub enabled: bool,
    #[serde(alias = "autoSync")]
    pub auto_sync: bool,
    #[serde(alias = "intervalHours")]
    pub interval_hours: u64,
    pub retention: usize,
}

impl Default for WebDavSettingsInput {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            username: String::new(),
            password: None,
            remote_path: "rweb-clash".into(),
            enabled: false,
            auto_sync: false,
            interval_hours: 24,
            retention: 7,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WebDavSettingsResponse {
    pub endpoint: String,
    pub username: String,
    #[serde(rename = "passwordConfigured")]
    pub password_configured: bool,
    #[serde(rename = "remotePath")]
    pub remote_path: String,
    pub enabled: bool,
    #[serde(rename = "autoSync")]
    pub auto_sync: bool,
    #[serde(rename = "intervalHours")]
    pub interval_hours: u64,
    pub retention: usize,
    #[serde(rename = "lastSync")]
    pub last_sync: Option<String>,
    #[serde(rename = "lastError")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupResponse {
    pub name: String,
    pub size: u64,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "remoteAvailable")]
    pub remote_available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoreStatusResponse {
    pub state: String,
    pub pid: Option<u32>,
    pub started_at: Option<String>,
    pub last_error: Option<String>,
    pub controller_addr: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemStatusResponse {
    pub core: CoreStatusResponse,
    pub config: SystemConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupStatusResponse {
    #[serde(rename = "needsOnboarding")]
    pub needs_onboarding: bool,
    #[serde(rename = "hasSubscriptions")]
    pub has_subscriptions: bool,
    #[serde(rename = "subscriptionCount")]
    pub subscription_count: usize,
    #[serde(rename = "hasSources")]
    pub has_sources: bool,
    #[serde(rename = "manualNodeCount")]
    pub manual_node_count: usize,
    #[serde(rename = "coreReady")]
    pub core_ready: bool,
    #[serde(rename = "corePath")]
    pub core_path: String,
    #[serde(rename = "mixedPortAvailable")]
    pub mixed_port_available: bool,
    #[serde(rename = "controllerPortAvailable")]
    pub controller_port_available: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct EgressResponse {
    pub ip: Option<String>,
    pub provider: Option<String>,
    pub country: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
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
}

pub fn enabled_default() -> bool {
    true
}
