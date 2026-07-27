use crate::error::AppError;
use crate::paths::{restrict_sensitive_file_permissions, AppPaths};
use crate::platform::current_system_proxy_url;
use crate::remote;
use crate::storage::{ProxyItemRecord, Storage, SubscriptionSyncCommit};
use crate::types::{
    FilterRule, DEFAULT_ACTIVE_PROBE_INTERVAL_SECONDS, DEFAULT_DELAY_TEST_URL,
    MAX_ACTIVE_PROBE_INTERVAL_SECONDS, MIN_ACTIVE_PROBE_INTERVAL_SECONDS, SUB_DELIMITER,
};
use crate::util::{content_hash, likely_country_from_name, new_id, now_iso, validate_url};
use base64::engine::general_purpose;
use base64::Engine;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::json;
use serde_yaml::Mapping;
use serde_yaml::Value;
use std::borrow::Cow;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::sync::{Arc, Weak};
use std::time::Instant;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, OwnedMutexGuard};
use tracing::{debug, info, warn};

const DEFAULT_USER_AGENT: &str = "rweb-clash/1.0 clash-verge";
const MAX_SUBSCRIPTION_BYTES: usize = 16 * 1024 * 1024;
const MAX_SUBSCRIPTION_NODES: usize = 20_000;
const MAX_SUBSCRIPTION_GROUPS: usize = 2_000;
const MAX_GROUP_MEMBERS: usize = 20_000;
const MAX_TOTAL_GROUP_MEMBERS: usize = 100_000;
const DEFAULT_PROBE_TOLERANCE_MS: i64 = 50;
const MIN_PROBE_TOLERANCE_MS: i64 = 0;
const MAX_PROBE_TOLERANCE_MS: i64 = 10_000;
const SUBSCRIPTION_CANDIDATE_PREFIX: &str = ".subscription-candidate-";
const SUBSCRIPTION_CANDIDATE_SUFFIX: &str = ".yaml";

#[derive(Debug, Clone)]
pub struct SubscriptionSyncer {
    storage: Storage,
    refresh_locks: Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>,
    paths: AppPaths,
}

#[derive(Debug, Clone, Default)]
struct SubscriptionMeta {
    upload: Option<u64>,
    download: Option<u64>,
    total: Option<u64>,
    expire: Option<String>,
}

#[derive(Debug, Clone)]
struct ParsedNode {
    original_name: String,
    runtime_name: String,
    protocol: String,
    country: Option<String>,
    raw_json: String,
    filtered_out: bool,
    filter_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct ParsedGroup {
    name: String,
    display_name: String,
    group_type: String,
    members: Vec<String>,
    url: Option<String>,
    interval: Option<i64>,
    tolerance: Option<i64>,
}

impl SubscriptionSyncer {
    pub fn new(storage: Storage, paths: AppPaths) -> Self {
        Self {
            storage,
            refresh_locks: Arc::new(Mutex::new(HashMap::new())),
            paths,
        }
    }

    pub async fn refresh(
        &self,
        subscription_id: &str,
        run_mihomo_validation: bool,
    ) -> Result<(), AppError> {
        self.storage.get_subscription_url(subscription_id).await?;
        let _refresh_guard = self.lock_refresh(subscription_id).await;
        self.refresh_locked(subscription_id, run_mihomo_validation)
            .await
    }

    pub(crate) async fn lock_refresh(&self, subscription_id: &str) -> OwnedMutexGuard<()> {
        let refresh_lock = {
            let mut locks = self.refresh_locks.lock().await;
            locks.retain(|_, refresh_lock| refresh_lock.strong_count() > 0);
            if let Some(refresh_lock) = locks.get(subscription_id).and_then(Weak::upgrade) {
                refresh_lock
            } else {
                let refresh_lock = Arc::new(Mutex::new(()));
                locks.insert(subscription_id.to_string(), Arc::downgrade(&refresh_lock));
                refresh_lock
            }
        };
        refresh_lock.lock_owned().await
    }

    pub(crate) async fn refresh_locked(
        &self,
        subscription_id: &str,
        run_mihomo_validation: bool,
    ) -> Result<(), AppError> {
        let (stored_name, url) = self.storage.get_subscription_url(subscription_id).await?;
        if !validate_url(&url) {
            let err = AppError::bad_request(
                "subscription_invalid_url",
                "subscription url must start with http:// or https://",
            );
            warn!(
                subscription_id = %subscription_id,
                url_host = %url_host_label(&url),
                "subscription refresh rejected invalid url"
            );
            self.storage
                .mark_subscription_sync_error(subscription_id, &err.message)
                .await?;
            return Err(err);
        }

        self.storage
            .mark_subscription_sync_start(subscription_id)
            .await?;
        let use_remote_name = stored_name.trim().is_empty();
        let fallback_name = if use_remote_name {
            generated_subscription_name()
        } else {
            stored_name.trim().to_string()
        };
        info!(
            subscription_id = %subscription_id,
            source_name = %fallback_name,
            remote_name_allowed = use_remote_name,
            url_host = %url_host_label(&url),
            "subscription refresh started"
        );
        let started = Instant::now();
        let result = self
            .refresh_inner(
                subscription_id,
                &fallback_name,
                use_remote_name,
                &url,
                run_mihomo_validation,
            )
            .await;
        if let Err(err) = &result {
            warn!(
                subscription_id = %subscription_id,
                error = %err,
                elapsed_ms = started.elapsed().as_millis(),
                "subscription refresh failed"
            );
            self.storage
                .mark_subscription_sync_error(subscription_id, &err.message)
                .await?;
        } else {
            info!(
                subscription_id = %subscription_id,
                elapsed_ms = started.elapsed().as_millis(),
                "subscription refresh completed"
            );
        }
        result
    }

    async fn refresh_inner(
        &self,
        subscription_id: &str,
        fallback_name: &str,
        use_remote_name: bool,
        url: &str,
        run_mihomo_validation: bool,
    ) -> Result<(), AppError> {
        info!(
            subscription_id = %subscription_id,
            url_host = %url_host_label(url),
            user_agent = DEFAULT_USER_AGENT,
            "fetching subscription"
        );
        let download_route = self
            .storage
            .get_subscription_download_route(subscription_id)
            .await?;
        let config = self.storage.load_config().await?;
        let system_proxy = match current_system_proxy_url().await {
            Ok(proxy) => proxy,
            Err(error) => {
                warn!(%error, "failed to inspect the system proxy download route");
                None
            }
        };
        let response = remote::get_text_routed(
            url,
            Some(DEFAULT_USER_AGENT),
            MAX_SUBSCRIPTION_BYTES,
            "subscription_fetch_failed",
            download_route,
            remote::RouteOptions {
                core_proxy: Some(format!("http://127.0.0.1:{}", config.mixed_port)),
                system_proxy,
            },
        )
        .await?;
        let used_route = response.route.clone();
        let status = response.status;
        let headers = response.headers;
        let body = response.body;
        info!(
            subscription_id = %subscription_id,
            status = status.as_u16(),
            bytes = body.len(),
            route = %used_route,
            "subscription fetched"
        );
        let meta = parse_subscription_meta(&headers);
        let source_name = if use_remote_name {
            subscription_name_from_headers(&headers).unwrap_or_else(|| fallback_name.to_string())
        } else {
            fallback_name.to_string()
        };
        if source_name != fallback_name {
            info!(
                subscription_id = %subscription_id,
                fallback_name = %fallback_name,
                source_name = %source_name,
                "subscription name updated from response headers"
            );
        }
        let (_inherit, global_rules, local_rules) = self
            .storage
            .subscription_rules_for_sync(subscription_id)
            .await?;
        let mut rules = Vec::new();
        rules.extend(global_rules);
        rules.extend(local_rules);
        let applied_rule_count = rules.len();
        let parse_subscription_id = subscription_id.to_string();
        let (body, source_format, parsed) = tokio::task::spawn_blocking(move || {
            let document = parse_subscription_document(&body)?;
            let parsed = parse_assets(&parse_subscription_id, &document.value, &rules)?;
            Ok::<_, AppError>((body, document.source_format, parsed))
        })
        .await
        .map_err(|error| {
            AppError::internal(format!("subscription parser task failed: {error}"))
        })??;
        let filtered_nodes = parsed.nodes.iter().filter(|node| node.filtered_out).count();
        info!(
            subscription_id = %subscription_id,
            source_name = %source_name,
            nodes = parsed.nodes.len(),
            filtered_nodes,
            groups = parsed.groups.len(),
            rules = applied_rule_count,
            "subscription parsed"
        );
        let mut items = Vec::with_capacity(parsed.nodes.len() + parsed.groups.len().max(1));
        let mut included_nodes = Vec::new();
        for (index, node) in parsed.nodes.iter().enumerate() {
            if !node.filtered_out {
                included_nodes.push(node.runtime_name.clone());
            }
            items.push(ProxyItemRecord {
                name: node.runtime_name.clone(),
                kind: "node".into(),
                subscription_id: Some(subscription_id.to_string()),
                display_name: node.original_name.clone(),
                source: "subscription".into(),
                builtin: false,
                source_name: Some(source_name.to_string()),
                protocol: Some(node.protocol.clone()),
                country: node.country.clone(),
                group_type: None,
                raw_json: Some(node.raw_json.clone()),
                content_hash: Some(content_hash(&node.raw_json)),
                latency_ms: None,
                alive: true,
                filtered_out: node.filtered_out,
                filter_reason: node.filter_reason.clone(),
                delay_ms: None,
                tolerance_ms: None,
                url: None,
                interval_seconds: None,
                strategy_json: "{}".into(),
                position: ((index + 1) as i64) * 1024,
                enabled: true,
            });
        }

        let groups = if parsed.groups.is_empty() && !included_nodes.is_empty() {
            vec![ParsedGroup {
                name: scoped_asset_name("全部节点", subscription_id),
                display_name: "全部节点".into(),
                group_type: "select".into(),
                members: included_nodes.clone(),
                url: None,
                interval: None,
                tolerance: None,
            }]
        } else {
            parsed.groups
        };

        let mut group_members = Vec::with_capacity(groups.len());
        for (index, group) in groups.iter().enumerate() {
            items.push(ProxyItemRecord {
                name: group.name.clone(),
                kind: "group".into(),
                subscription_id: Some(subscription_id.to_string()),
                display_name: group.display_name.clone(),
                source: "subscription".into(),
                builtin: false,
                source_name: Some(source_name.to_string()),
                protocol: None,
                country: None,
                group_type: Some(group.group_type.clone()),
                raw_json: None,
                content_hash: None,
                latency_ms: None,
                alive: true,
                filtered_out: false,
                filter_reason: None,
                delay_ms: None,
                tolerance_ms: group.tolerance,
                url: group.url.clone(),
                interval_seconds: group.interval,
                strategy_json: json!({ "now": group.members.first() }).to_string(),
                position: ((index + 1) as i64) * 1024,
                enabled: true,
            });
            group_members.push((group.name.clone(), group.members.clone()));
        }

        self.validate_and_replace_assets(
            subscription_id,
            &items,
            &group_members,
            SubscriptionSyncCommit {
                subscription_name: source_name.clone(),
                node_count: included_nodes.len() as i64,
                upload_bytes: meta.upload,
                download_bytes: meta.download,
                total_bytes: meta.total,
                expire_at: meta.expire,
                source_format: source_format.to_string(),
                raw_content_hash: content_hash(&body),
            },
            run_mihomo_validation,
        )
        .await?;
        self.storage
            .set_subscription_last_route(subscription_id, &used_route)
            .await?;
        info!(
            subscription_id = %subscription_id,
            source_name = %source_name,
            included_nodes = included_nodes.len(),
            "subscription assets saved"
        );
        Ok(())
    }

    async fn validate_and_replace_assets(
        &self,
        subscription_id: &str,
        items: &[ProxyItemRecord],
        group_members: &[(String, Vec<String>)],
        commit: SubscriptionSyncCommit,
        run_mihomo_validation: bool,
    ) -> Result<(), AppError> {
        let candidate_yaml = crate::runtime::subscription_candidate_yaml(items, group_members)?;
        validate_subscription_candidate(&self.paths, &candidate_yaml, run_mihomo_validation)
            .await?;
        self.storage
            .replace_subscription_assets(subscription_id, items, group_members, commit)
            .await?;
        Ok(())
    }
}

async fn validate_subscription_candidate(
    paths: &AppPaths,
    candidate_yaml: &str,
    run_mihomo_validation: bool,
) -> Result<(), AppError> {
    validate_subscription_candidate_structure(candidate_yaml)?;
    if !run_mihomo_validation {
        debug!("core is stopped; subscription candidate passed structural validation");
        return Ok(());
    }
    let mihomo_binary = paths.mihomo_binary();
    if !mihomo_binary.is_file() {
        warn!(
            mihomo_binary = %AppPaths::display(&mihomo_binary),
            "mihomo is unavailable; subscription candidate passed conservative structural validation"
        );
        return Ok(());
    }
    let candidate_file = write_subscription_candidate(paths, candidate_yaml).await?;
    crate::core::validate_mihomo_config(&mihomo_binary, &paths.profiles_dir, candidate_file.path())
        .await
}

async fn write_subscription_candidate(
    paths: &AppPaths,
    candidate_yaml: &str,
) -> Result<CandidateConfigFile, AppError> {
    let candidate_path = paths.profiles_dir.join(format!(
        "{SUBSCRIPTION_CANDIDATE_PREFIX}{}{SUBSCRIPTION_CANDIDATE_SUFFIX}",
        new_id("validation"),
    ));
    let candidate_file = CandidateConfigFile(candidate_path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(crate::paths::PRIVATE_FILE_MODE);
    let file = options.open(candidate_file.path())?;
    restrict_sensitive_file_permissions(candidate_file.path())?;
    let mut file = tokio::fs::File::from_std(file);
    file.write_all(candidate_yaml.as_bytes()).await?;
    file.flush().await?;
    file.sync_all().await?;
    Ok(candidate_file)
}

pub(crate) fn cleanup_stale_subscription_candidates(paths: &AppPaths) -> usize {
    let entries = match std::fs::read_dir(&paths.profiles_dir) {
        Ok(entries) => entries,
        Err(error) => {
            warn!(
                %error,
                profiles_dir = %AppPaths::display(&paths.profiles_dir),
                "failed to scan for stale subscription candidate files"
            );
            return 0;
        }
    };

    let mut removed = 0;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                warn!(%error, "failed to inspect a subscription candidate directory entry");
                continue;
            }
        };
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !is_subscription_candidate_file_name(file_name) {
            continue;
        }
        match entry.file_type() {
            Ok(file_type) if file_type.is_file() => {}
            Ok(_) => continue,
            Err(error) => {
                warn!(
                    %error,
                    path = %AppPaths::display(&entry.path()),
                    "failed to inspect a stale subscription candidate file"
                );
                continue;
            }
        }
        match std::fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(error) => warn!(
                %error,
                path = %AppPaths::display(&entry.path()),
                "failed to remove a stale subscription candidate file"
            ),
        }
    }

    if removed > 0 {
        info!(removed, "removed stale subscription candidate files");
    }
    removed
}

fn is_subscription_candidate_file_name(file_name: &str) -> bool {
    file_name
        .strip_prefix(SUBSCRIPTION_CANDIDATE_PREFIX)
        .and_then(|name| name.strip_suffix(SUBSCRIPTION_CANDIDATE_SUFFIX))
        .is_some_and(|name| !name.is_empty())
}

struct CandidateConfigFile(std::path::PathBuf);

impl CandidateConfigFile {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for CandidateConfigFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn validate_subscription_candidate_structure(candidate_yaml: &str) -> Result<(), AppError> {
    const SUPPORTED_TYPES: &[&str] = &[
        "ss",
        "ssr",
        "vmess",
        "vless",
        "trojan",
        "snell",
        "socks5",
        "http",
        "hysteria",
        "hysteria2",
        "tuic",
        "wireguard",
        "ssh",
        "mieru",
        "anytls",
    ];
    let root = serde_yaml::from_str::<Value>(candidate_yaml)?;
    let proxies = root
        .as_mapping()
        .and_then(|mapping| mapping.get(Value::String("proxies".into())))
        .and_then(Value::as_sequence)
        .ok_or_else(|| {
            AppError::bad_request(
                "subscription_candidate_invalid",
                "subscription candidate has no proxy list",
            )
        })?;
    for proxy in proxies {
        let name = yaml_field_string(proxy, "name").unwrap_or_else(|| "<unnamed>".into());
        let proxy_type = yaml_field_string(proxy, "type")
            .ok_or_else(|| invalid_candidate_node(&name, "has no proxy type"))?;
        if !SUPPORTED_TYPES.contains(&proxy_type.as_str()) {
            return Err(invalid_candidate_node(
                &name,
                &format!("uses unsupported proxy type {proxy_type}"),
            ));
        }
        if yaml_field_string(proxy, "server").is_none_or(|server| server.trim().is_empty()) {
            return Err(invalid_candidate_node(&name, "has no server"));
        }
        let has_port =
            yaml_field_i64(proxy, "port").is_some_and(|port| (1..=65_535).contains(&port));
        let has_port_range = (matches!(proxy_type.as_str(), "hysteria" | "hysteria2")
            && yaml_field_string(proxy, "ports").is_some_and(|ports| !ports.trim().is_empty()))
            || (proxy_type == "mieru"
                && yaml_field_string(proxy, "port-range")
                    .is_some_and(|ports| !ports.trim().is_empty()));
        if !has_port && !has_port_range {
            return Err(invalid_candidate_node(&name, "has no valid port"));
        }
        validate_fallback_proxy_fields(proxy, &name, &proxy_type)?;
    }
    Ok(())
}

fn validate_fallback_proxy_fields(
    proxy: &Value,
    name: &str,
    proxy_type: &str,
) -> Result<(), AppError> {
    let required: &[&str] = match proxy_type {
        "ss" => &["cipher", "password"],
        "ssr" => &["cipher", "password", "protocol", "obfs"],
        "vmess" | "vless" => &["uuid"],
        "trojan" | "hysteria2" | "mieru" | "anytls" => &["password"],
        "snell" => &["psk"],
        "wireguard" => &["ip", "private-key", "public-key"],
        "ssh" => &["username"],
        _ => &[],
    };
    for field in required {
        if !candidate_field_has_value(proxy, field) {
            return Err(invalid_candidate_node(name, &format!("has no {field}")));
        }
    }
    if proxy_type == "tuic"
        && !candidate_field_has_value(proxy, "token")
        && !(candidate_field_has_value(proxy, "uuid")
            && candidate_field_has_value(proxy, "password"))
    {
        return Err(invalid_candidate_node(
            name,
            "has neither token nor uuid/password authentication",
        ));
    }
    if proxy_type == "ssh"
        && !candidate_field_has_value(proxy, "password")
        && !candidate_field_has_value(proxy, "private-key")
    {
        return Err(invalid_candidate_node(
            name,
            "has neither password nor private-key authentication",
        ));
    }
    Ok(())
}

fn candidate_field_has_value(proxy: &Value, field: &str) -> bool {
    proxy
        .as_mapping()
        .and_then(|mapping| mapping.get(Value::String(field.into())))
        .is_some_and(|value| match value {
            Value::Null => false,
            Value::String(value) => !value.trim().is_empty(),
            Value::Sequence(value) => !value.is_empty(),
            _ => true,
        })
}

fn invalid_candidate_node(name: &str, reason: &str) -> AppError {
    AppError::bad_request(
        "subscription_candidate_invalid",
        format!("subscription node {name} {reason}"),
    )
}

fn url_host_label(value: &str) -> String {
    reqwest::Url::parse(value)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into())
}

#[derive(Debug)]
struct ParsedAssets {
    nodes: Vec<ParsedNode>,
    groups: Vec<ParsedGroup>,
}

#[derive(Debug, Clone)]
struct ParsedSubscriptionDocument {
    value: Value,
    source_format: &'static str,
}

fn parse_subscription_document(body: &str) -> Result<ParsedSubscriptionDocument, AppError> {
    if let Some(value) = parse_clash_yaml(body) {
        return Ok(ParsedSubscriptionDocument {
            value,
            source_format: "clash",
        });
    }

    let decoded_candidates = decode_base64_text_candidates(body);
    for decoded in &decoded_candidates {
        if let Some(value) = parse_clash_yaml(decoded) {
            return Ok(ParsedSubscriptionDocument {
                value,
                source_format: "base64-clash",
            });
        }
    }

    if let Some(value) = parse_legacy_subscription_document(body) {
        return Ok(ParsedSubscriptionDocument {
            value,
            source_format: "legacy",
        });
    }

    for decoded in &decoded_candidates {
        if let Some(value) = parse_legacy_subscription_document(decoded) {
            return Ok(ParsedSubscriptionDocument {
                value,
                source_format: "base64-legacy",
            });
        }
    }

    Err(AppError::bad_request(
        "subscription_parse_failed",
        "subscription is neither a Clash YAML profile nor a supported legacy URI subscription",
    ))
}

fn parse_clash_yaml(body: &str) -> Option<Value> {
    serde_yaml::from_str::<Value>(body)
        .ok()
        .filter(|value| value.as_mapping().is_some())
}

fn parse_legacy_subscription_document(body: &str) -> Option<Value> {
    let mut nodes = Vec::new();
    let mut names = HashSet::new();
    for line in body.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parsed = match line.split_once("://").map(|(scheme, _)| scheme) {
            Some("ss") => parse_ss_uri(line),
            Some("ssr") => parse_ssr_uri(line),
            Some("trojan") => parse_trojan_uri(line),
            Some("vmess") => parse_vmess_uri(line),
            Some("vless") => parse_vless_uri(line),
            _ => None,
        };
        if let Some(mut node) = parsed {
            let name = yaml_field_string(&node, "name")
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| format!("legacy-{}", nodes.len() + 1));
            set_yaml_field(
                &mut node,
                "name",
                Value::String(unique_legacy_node_name(&name, &mut names)),
            );
            nodes.push(node);
        }
    }
    if nodes.is_empty() {
        return None;
    }

    let mut root = Mapping::new();
    root.insert(
        Value::String("proxies".into()),
        Value::Sequence(nodes.into_iter().collect()),
    );
    Some(Value::Mapping(root))
}

fn parse_ss_uri(line: &str) -> Option<Value> {
    let raw = line.strip_prefix("ss://")?;
    let (without_fragment, name) = split_fragment(raw);
    let (main, _query) = split_query(without_fragment);
    let (userinfo, server) = if let Some((left, right)) = main.rsplit_once('@') {
        let decoded_left =
            decode_base64_segment(left).unwrap_or_else(|| percent_decode_lossy(left));
        (decoded_left, right.to_string())
    } else {
        let decoded = decode_base64_segment(main)?;
        let (left, right) = decoded.rsplit_once('@')?;
        (left.to_string(), right.to_string())
    };
    let (cipher, password) = userinfo.split_once(':')?;
    let (server, port) = parse_host_port(&server)?;

    let mut node = legacy_node_base("ss", &name.unwrap_or_else(|| server.clone()), &server, port);
    insert_yaml_string(&mut node, "cipher", cipher);
    insert_yaml_string(&mut node, "password", &percent_decode_lossy(password));
    Some(Value::Mapping(node))
}

fn parse_ssr_uri(line: &str) -> Option<Value> {
    let payload = line.strip_prefix("ssr://")?;
    let decoded = decode_base64_segment(payload)?;
    let (main, query) = decoded.split_once("/?").unwrap_or((&decoded, ""));
    let mut parts = main.splitn(6, ':').collect::<Vec<_>>();
    if parts.len() != 6 {
        return None;
    }
    let server = parts.remove(0).to_string();
    let port = parts.remove(0).parse::<i64>().ok()?;
    let protocol = parts.remove(0);
    let cipher = parts.remove(0);
    let obfs = parts.remove(0);
    let password = decode_base64_segment(parts.remove(0))?;
    let params = parse_query_map(query);
    let name = params
        .get("remarks")
        .and_then(|value| decode_base64_segment(value))
        .unwrap_or_else(|| server.clone());

    let mut node = legacy_node_base("ssr", &name, &server, port);
    insert_yaml_string(&mut node, "cipher", cipher);
    insert_yaml_string(&mut node, "password", &password);
    insert_yaml_string(&mut node, "protocol", protocol);
    insert_yaml_string(&mut node, "obfs", obfs);
    if let Some(value) = params
        .get("protoparam")
        .and_then(|value| decode_base64_segment(value))
    {
        insert_yaml_string(&mut node, "protocol-param", &value);
    }
    if let Some(value) = params
        .get("obfsparam")
        .and_then(|value| decode_base64_segment(value))
    {
        insert_yaml_string(&mut node, "obfs-param", &value);
    }
    Some(Value::Mapping(node))
}

fn parse_trojan_uri(line: &str) -> Option<Value> {
    let raw = line.strip_prefix("trojan://")?;
    let (without_fragment, name) = split_fragment(raw);
    let (main, query) = split_query(without_fragment);
    let (password, server_part) = main.split_once('@')?;
    let (server, port) = parse_host_port(server_part)?;
    let params = parse_query_map(query);

    let mut node = legacy_node_base(
        "trojan",
        &name.unwrap_or_else(|| server.clone()),
        &server,
        port,
    );
    insert_yaml_string(&mut node, "password", &percent_decode_lossy(password));
    if let Some(value) = first_query_value(&params, &["sni", "peer", "host"]) {
        insert_yaml_string(&mut node, "sni", value);
    }
    if query_bool(&params, "allowInsecure") || query_bool(&params, "skip-cert-verify") {
        insert_yaml_bool(&mut node, "skip-cert-verify", true);
    }
    Some(Value::Mapping(node))
}

fn parse_vmess_uri(line: &str) -> Option<Value> {
    let payload = line.strip_prefix("vmess://")?;
    let decoded = decode_base64_segment(payload)?;
    let value: serde_json::Value = serde_json::from_str(&decoded).ok()?;
    let object = value.as_object()?;
    let server = json_string(object, "add")?;
    let port = json_i64(object, "port")?;
    let uuid = json_string(object, "id")?;
    let name = json_string(object, "ps").unwrap_or_else(|| server.clone());

    let mut node = legacy_node_base("vmess", &name, &server, port);
    insert_yaml_string(&mut node, "uuid", &uuid);
    insert_yaml_string(
        &mut node,
        "cipher",
        &json_string(object, "scy")
            .or_else(|| json_string(object, "cipher"))
            .unwrap_or_else(|| "auto".into()),
    );
    insert_yaml_i64(&mut node, "alterId", json_i64(object, "aid").unwrap_or(0));

    let network = json_string(object, "net").unwrap_or_else(|| "tcp".into());
    if !network.is_empty() && network != "tcp" {
        insert_yaml_string(&mut node, "network", &network);
    }
    let tls = json_string(object, "tls")
        .map(|value| value.eq_ignore_ascii_case("tls"))
        .unwrap_or(false);
    if tls {
        insert_yaml_bool(&mut node, "tls", true);
    }
    if let Some(servername) = json_string(object, "sni").or_else(|| json_string(object, "host")) {
        if !servername.trim().is_empty() {
            insert_yaml_string(&mut node, "servername", &servername);
        }
    }
    if network == "ws" {
        insert_ws_opts(
            &mut node,
            json_string(object, "path").as_deref(),
            json_string(object, "host").as_deref(),
        );
    }
    Some(Value::Mapping(node))
}

fn parse_vless_uri(line: &str) -> Option<Value> {
    let raw = line.strip_prefix("vless://")?;
    let (without_fragment, name) = split_fragment(raw);
    let (main, query) = split_query(without_fragment);
    let (uuid, server_part) = main.split_once('@')?;
    let (server, port) = parse_host_port(server_part)?;
    let params = parse_query_map(query);

    let mut node = legacy_node_base(
        "vless",
        &name.unwrap_or_else(|| server.clone()),
        &server,
        port,
    );
    insert_yaml_string(&mut node, "uuid", &percent_decode_lossy(uuid));
    insert_yaml_string(
        &mut node,
        "encryption",
        first_query_value(&params, &["encryption"]).unwrap_or("none"),
    );
    if let Some(network) = first_query_value(&params, &["type"]) {
        if network != "tcp" {
            insert_yaml_string(&mut node, "network", network);
        }
    }
    if let Some(flow) = first_query_value(&params, &["flow"]) {
        insert_yaml_string(&mut node, "flow", flow);
    }
    let security = first_query_value(&params, &["security"]).unwrap_or("none");
    if matches!(security, "tls" | "reality") {
        insert_yaml_bool(&mut node, "tls", true);
    }
    if let Some(servername) = first_query_value(&params, &["sni", "peer", "host"]) {
        insert_yaml_string(&mut node, "servername", servername);
    }
    if first_query_value(&params, &["type"]) == Some("ws") {
        insert_ws_opts(
            &mut node,
            first_query_value(&params, &["path"]),
            first_query_value(&params, &["host"]),
        );
    }
    if security == "reality" {
        let mut reality = Mapping::new();
        if let Some(value) = first_query_value(&params, &["pbk", "public-key"]) {
            insert_yaml_string(&mut reality, "public-key", value);
        }
        if let Some(value) = first_query_value(&params, &["sid", "short-id"]) {
            insert_yaml_string(&mut reality, "short-id", value);
        }
        if let Some(value) = first_query_value(&params, &["spx", "spider-x"]) {
            insert_yaml_string(&mut reality, "spider-x", value);
        }
        if !reality.is_empty() {
            node.insert(
                Value::String("reality-opts".into()),
                Value::Mapping(reality),
            );
        }
    }
    if let Some(value) = first_query_value(&params, &["fp"]) {
        insert_yaml_string(&mut node, "client-fingerprint", value);
    }
    Some(Value::Mapping(node))
}

fn legacy_node_base(protocol: &str, name: &str, server: &str, port: i64) -> Mapping {
    let mut node = Mapping::new();
    insert_yaml_string(&mut node, "name", name);
    insert_yaml_string(&mut node, "type", protocol);
    insert_yaml_string(&mut node, "server", server);
    insert_yaml_i64(&mut node, "port", port);
    node
}

fn unique_legacy_node_name(name: &str, used: &mut HashSet<String>) -> String {
    let base = name.trim();
    let base = if base.is_empty() { "legacy-node" } else { base };
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    for index in 2.. {
        let candidate = format!("{base} {index}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

fn split_fragment(value: &str) -> (&str, Option<String>) {
    let Some((main, fragment)) = value.split_once('#') else {
        return (value, None);
    };
    let name = percent_decode_lossy(fragment).trim().to_string();
    if name.is_empty() {
        (main, None)
    } else {
        (main, Some(name))
    }
}

fn split_query(value: &str) -> (&str, &str) {
    value.split_once('?').unwrap_or((value, ""))
}

fn decode_base64_text_candidates(value: &str) -> Vec<String> {
    let compact = value.split_whitespace().collect::<String>();
    let mut output = Vec::new();
    for engine in base64_engines() {
        let Ok(decoded) = engine.decode(compact.as_bytes()) else {
            continue;
        };
        let Ok(text) = String::from_utf8(decoded) else {
            continue;
        };
        if !text.trim().is_empty() && !output.iter().any(|existing| existing == &text) {
            output.push(text);
        }
    }
    output
}

fn decode_base64_segment(value: &str) -> Option<String> {
    let compact = value.split_whitespace().collect::<String>();
    if compact.is_empty() {
        return None;
    }
    for engine in base64_engines() {
        if let Ok(decoded) = engine.decode(compact.as_bytes()) {
            if let Ok(text) = String::from_utf8(decoded) {
                return Some(text);
            }
        }
    }
    None
}

fn base64_engines() -> [&'static base64::engine::GeneralPurpose; 4] {
    [
        &general_purpose::STANDARD,
        &general_purpose::STANDARD_NO_PAD,
        &general_purpose::URL_SAFE,
        &general_purpose::URL_SAFE_NO_PAD,
    ]
}

fn parse_host_port(value: &str) -> Option<(String, i64)> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix('[') {
        let (host, rest) = rest.split_once(']')?;
        let port = rest.strip_prefix(':')?.parse::<i64>().ok()?;
        return Some((host.to_string(), port));
    }
    let (host, port) = value.rsplit_once(':')?;
    Some((percent_decode_lossy(host), port.parse::<i64>().ok()?))
}

fn parse_query_map(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for part in query.split('&') {
        if part.trim().is_empty() {
            continue;
        }
        let (key, value) = part.split_once('=').unwrap_or((part, ""));
        map.insert(percent_decode_lossy(key), percent_decode_lossy(value));
    }
    map
}

fn first_query_value<'a>(params: &'a HashMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| params.get(*key).map(String::as_str))
        .filter(|value| !value.trim().is_empty())
}

fn query_bool(params: &HashMap<String, String>, key: &str) -> bool {
    params
        .get(key)
        .map(|value| matches!(value.as_str(), "1" | "true" | "True" | "TRUE"))
        .unwrap_or(false)
}

fn json_string(object: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    object.get(key).and_then(|value| match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

fn json_i64(object: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<i64> {
    object.get(key).and_then(|value| match value {
        serde_json::Value::Number(number) => number.as_i64(),
        serde_json::Value::String(text) => text.parse::<i64>().ok(),
        _ => None,
    })
}

fn insert_ws_opts(node: &mut Mapping, path: Option<&str>, host: Option<&str>) {
    let mut opts = Mapping::new();
    if let Some(path) = path.filter(|value| !value.trim().is_empty()) {
        insert_yaml_string(&mut opts, "path", path);
    }
    if let Some(host) = host.filter(|value| !value.trim().is_empty()) {
        let mut headers = Mapping::new();
        insert_yaml_string(&mut headers, "Host", host);
        opts.insert(Value::String("headers".into()), Value::Mapping(headers));
    }
    if !opts.is_empty() {
        node.insert(Value::String("ws-opts".into()), Value::Mapping(opts));
    }
}

fn insert_yaml_string(mapping: &mut Mapping, key: &str, value: &str) {
    mapping.insert(
        Value::String(key.to_string()),
        Value::String(percent_decode_lossy(value)),
    );
}

fn insert_yaml_i64(mapping: &mut Mapping, key: &str, value: i64) {
    mapping.insert(Value::String(key.to_string()), Value::Number(value.into()));
}

fn insert_yaml_bool(mapping: &mut Mapping, key: &str, value: bool) {
    mapping.insert(Value::String(key.to_string()), Value::Bool(value));
}

fn percent_decode_lossy(value: &str) -> String {
    urlencoding::decode(value)
        .map(Cow::into_owned)
        .unwrap_or_else(|_| value.to_string())
}

fn parse_assets(
    subscription_id: &str,
    yaml: &Value,
    rules: &[FilterRule],
) -> Result<ParsedAssets, AppError> {
    let root = yaml.as_mapping().ok_or_else(|| {
        AppError::bad_request(
            "subscription_parse_failed",
            "subscription root must be a YAML object",
        )
    })?;
    let proxies = root
        .get(Value::String("proxies".into()))
        .and_then(Value::as_sequence)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let proxy_groups = root
        .get(Value::String("proxy-groups".into()))
        .and_then(Value::as_sequence)
        .map(Vec::as_slice)
        .unwrap_or_default();
    enforce_subscription_limit("nodes", proxies.len(), MAX_SUBSCRIPTION_NODES)?;
    enforce_subscription_limit("groups", proxy_groups.len(), MAX_SUBSCRIPTION_GROUPS)?;
    let mut total_members = 0usize;
    for group in proxy_groups {
        let member_count = yaml_field_sequence_ref(group, "proxies").map_or(0, <[_]>::len);
        enforce_subscription_limit("members in one group", member_count, MAX_GROUP_MEMBERS)?;
        total_members = total_members.saturating_add(member_count);
        enforce_subscription_limit(
            "members across all groups",
            total_members,
            MAX_TOTAL_GROUP_MEMBERS,
        )?;
    }
    let proxies = proxies.to_vec();
    let proxy_groups = proxy_groups.to_vec();

    let mut used_names = HashSet::new();
    let mut node_name_map = HashMap::new();
    let mut group_name_map = HashMap::new();

    for node in &proxies {
        if let Some(name) = yaml_field_string(node, "name") {
            if let Entry::Vacant(entry) = node_name_map.entry(name.clone()) {
                let runtime = scoped_name(&name, subscription_id, &mut used_names);
                entry.insert(runtime);
            }
        }
    }
    for group in &proxy_groups {
        if let Some(name) = yaml_field_string(group, "name") {
            if let Entry::Vacant(entry) = group_name_map.entry(name.clone()) {
                let runtime = scoped_name(&name, subscription_id, &mut used_names);
                entry.insert(runtime);
            }
        }
    }

    let has_keep_rules = rules
        .iter()
        .any(|rule| rule.enabled && matches!(rule.action.as_str(), "keep" | "include" | "引入"));

    let mut parsed_nodes = Vec::with_capacity(proxies.len());
    let mut seen_nodes = HashSet::new();
    for node in proxies {
        let Some(original_name) = yaml_field_string(&node, "name") else {
            continue;
        };
        let runtime_name = if seen_nodes.insert(original_name.clone()) {
            node_name_map
                .get(&original_name)
                .cloned()
                .unwrap_or_else(|| scoped_asset_name(&original_name, subscription_id))
        } else {
            scoped_name(&original_name, subscription_id, &mut used_names)
        };
        let protocol = yaml_field_string(&node, "type").unwrap_or_else(|| "unknown".into());
        let mut raw = node.clone();
        set_yaml_field(&mut raw, "name", Value::String(runtime_name.clone()));
        let raw_json = serde_json::to_string(&raw).map_err(AppError::from)?;
        let (included, reason) = apply_filter_rules(&original_name, rules, has_keep_rules);

        parsed_nodes.push(ParsedNode {
            original_name,
            runtime_name,
            protocol,
            country: likely_country_from_name(&raw_json),
            raw_json,
            filtered_out: !included,
            filter_reason: reason,
        });
    }

    let available_nodes = parsed_nodes
        .iter()
        .filter(|node| !node.filtered_out)
        .map(|node| node.runtime_name.clone())
        .collect::<HashSet<_>>();
    let available_groups = group_name_map
        .values()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut parsed_groups = Vec::new();
    let mut seen_groups = HashSet::new();
    for group in proxy_groups {
        let Some(original_name) = yaml_field_string(&group, "name") else {
            continue;
        };
        let name = if seen_groups.insert(original_name.clone()) {
            group_name_map
                .get(&original_name)
                .cloned()
                .unwrap_or_else(|| scoped_asset_name(&original_name, subscription_id))
        } else {
            scoped_name(&original_name, subscription_id, &mut used_names)
        };
        let group_type = normalize_subscription_group_type(&group, &original_name)?;
        let active_probe = is_active_probe_group(&group_type);
        let members = yaml_field_sequence(&group, "proxies")
            .into_iter()
            .filter_map(|member| {
                let member = member.as_str()?.trim();
                Some(
                    node_name_map
                        .get(member)
                        .or_else(|| group_name_map.get(member))
                        .cloned()
                        .unwrap_or_else(|| member.to_string()),
                )
            })
            .filter(|member| {
                let builtin = is_builtin_group_member(member);
                (!active_probe && builtin)
                    || available_nodes.contains(member.as_str())
                    || available_groups.contains(member.as_str())
            })
            .collect::<Vec<_>>();
        if members.is_empty() {
            continue;
        }
        let (url, interval, tolerance) = normalized_subscription_group_probe(&group, &group_type);
        parsed_groups.push(ParsedGroup {
            name,
            display_name: original_name,
            group_type,
            members,
            url,
            interval,
            tolerance,
        });
    }

    if parsed_nodes.is_empty() {
        return Err(AppError::bad_request(
            "subscription_parse_failed",
            format!("subscription {subscription_id} did not contain any proxies"),
        ));
    }
    let member_map = parsed_groups
        .iter()
        .map(|group| (group.name.clone(), group.members.clone()))
        .collect::<HashMap<_, _>>();
    crate::runtime::resolve_available_groups(&member_map, &available_nodes)?;

    Ok(ParsedAssets {
        nodes: parsed_nodes,
        groups: parsed_groups,
    })
}

fn normalize_subscription_group_type(group: &Value, name: &str) -> Result<String, AppError> {
    let group_type = yaml_field_string(group, "type").unwrap_or_else(|| "select".into());
    let group_type = group_type.trim();
    if matches!(
        group_type,
        "select" | "url-test" | "fallback" | "load-balance"
    ) {
        return Ok(group_type.to_string());
    }
    Err(AppError::bad_request(
        "subscription_unsupported_group_type",
        format!("subscription proxy group {name} uses unsupported type {group_type}"),
    ))
}

fn is_active_probe_group(group_type: &str) -> bool {
    matches!(group_type, "url-test" | "fallback" | "load-balance")
}

fn is_builtin_group_member(member: &str) -> bool {
    matches!(member, "DIRECT" | "REJECT" | "GLOBAL" | "PROXY")
}

fn normalized_subscription_group_probe(
    group: &Value,
    group_type: &str,
) -> (Option<String>, Option<i64>, Option<i64>) {
    if !is_active_probe_group(group_type) {
        return (None, None, None);
    }
    let interval = yaml_field_i64(group, "interval")
        .unwrap_or(DEFAULT_ACTIVE_PROBE_INTERVAL_SECONDS)
        .clamp(
            MIN_ACTIVE_PROBE_INTERVAL_SECONDS,
            MAX_ACTIVE_PROBE_INTERVAL_SECONDS,
        );
    let tolerance = (group_type == "url-test").then(|| {
        yaml_field_i64(group, "tolerance")
            .unwrap_or(DEFAULT_PROBE_TOLERANCE_MS)
            .clamp(MIN_PROBE_TOLERANCE_MS, MAX_PROBE_TOLERANCE_MS)
    });
    (
        Some(DEFAULT_DELAY_TEST_URL.to_string()),
        Some(interval),
        tolerance,
    )
}

fn apply_filter_rules(
    node_name: &str,
    rules: &[FilterRule],
    has_keep_rules: bool,
) -> (bool, Option<String>) {
    let mut included = !has_keep_rules;
    let mut reason = None;
    for rule in rules.iter().filter(|rule| rule.enabled) {
        let matched = match_rule(node_name, rule);
        if !matched {
            continue;
        }
        let label = rule_label(rule);
        match rule.action.as_str() {
            "keep" | "include" | "引入" => {
                included = true;
                reason = Some(format!("matched keep rule {label}"));
            }
            _ => {
                included = false;
                reason = Some(format!("matched discard rule {label}"));
            }
        }
    }
    if included {
        (true, reason)
    } else {
        (
            false,
            reason.or_else(|| Some("not matched by keep rules".into())),
        )
    }
}

fn rule_label(rule: &FilterRule) -> String {
    if rule.match_type == "in" && rule.has_values() {
        rule.effective_values().join(", ")
    } else {
        rule.pattern.clone()
    }
}

fn match_rule(node_name: &str, rule: &FilterRule) -> bool {
    let lower = node_name.to_ascii_lowercase();
    let pattern_lower = rule.pattern.to_ascii_lowercase();
    match rule.match_type.as_str() {
        "in" | "equals" => rule
            .effective_values()
            .into_iter()
            .any(|value| node_name.eq_ignore_ascii_case(value)),
        "not_contains" | "notContains" => !lower.contains(&pattern_lower),
        "regex" => Regex::new(&rule.pattern)
            .map(|regex| regex.is_match(node_name))
            .unwrap_or(false),
        _ => lower.contains(&pattern_lower),
    }
}

fn enforce_subscription_limit(label: &str, actual: usize, limit: usize) -> Result<(), AppError> {
    if actual <= limit {
        return Ok(());
    }
    Err(AppError::bad_request(
        "subscription_limit_exceeded",
        format!("subscription contains {actual} {label}; maximum is {limit}"),
    ))
}

fn scoped_asset_name(name: &str, subscription_id: &str) -> String {
    format!("{}{SUB_DELIMITER}{}", name.trim(), subscription_id.trim())
}

fn scoped_name(name: &str, subscription_id: &str, used: &mut HashSet<String>) -> String {
    let base = scoped_asset_name(name, subscription_id);
    if used.insert(base.clone()) {
        return base;
    }
    for duplicate_index in 2_u64.. {
        let candidate = format!("{base}{SUB_DELIMITER}{duplicate_index}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("the duplicate index space cannot be exhausted")
}

fn yaml_field_string(value: &Value, field: &str) -> Option<String> {
    value
        .as_mapping()?
        .get(Value::String(field.to_string()))?
        .as_str()
        .map(str::to_string)
}

fn yaml_field_i64(value: &Value, field: &str) -> Option<i64> {
    value
        .as_mapping()?
        .get(Value::String(field.to_string()))?
        .as_i64()
}

fn yaml_field_sequence(value: &Value, field: &str) -> Vec<Value> {
    yaml_field_sequence_ref(value, field)
        .map(<[Value]>::to_vec)
        .unwrap_or_default()
}

fn yaml_field_sequence_ref<'a>(value: &'a Value, field: &str) -> Option<&'a [Value]> {
    value
        .as_mapping()
        .and_then(|mapping| mapping.get(Value::String(field.to_string())))
        .and_then(Value::as_sequence)
        .map(Vec::as_slice)
}

fn set_yaml_field(value: &mut Value, field: &str, replacement: Value) {
    if let Some(mapping) = value.as_mapping_mut() {
        mapping.insert(Value::String(field.to_string()), replacement);
    }
}

fn parse_subscription_meta(headers: &HeaderMap) -> SubscriptionMeta {
    let mut meta = SubscriptionMeta::default();
    if let Some(value) = headers.get("subscription-userinfo") {
        parse_subscription_userinfo(value, &mut meta);
    }
    if meta.expire.is_none() {
        meta.expire = headers
            .get("profile-expire")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
    }
    meta
}

fn subscription_name_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("profile-title")
        .and_then(parse_profile_title)
        .or_else(|| {
            headers
                .get(reqwest::header::CONTENT_DISPOSITION)
                .and_then(parse_content_disposition_filename)
        })
}

fn parse_profile_title(value: &HeaderValue) -> Option<String> {
    let raw = trim_header_value(value.to_str().ok()?);
    let lower = raw.to_ascii_lowercase();
    if let Some(payload) = lower
        .strip_prefix("base64:")
        .and_then(|_| raw.split_once(':').map(|(_, payload)| payload.trim()))
    {
        return percent_decode(payload)
            .and_then(|decoded| decode_base64_text(&decoded))
            .or_else(|| decode_base64_text(payload));
    }

    percent_decode(raw)
        .and_then(|decoded| sanitize_subscription_name(&decoded))
        .or_else(|| decode_base64_text(raw))
        .or_else(|| sanitize_subscription_name(raw))
}

fn parse_content_disposition_filename(value: &HeaderValue) -> Option<String> {
    let raw = value.to_str().ok()?;
    let mut filename = None;
    for part in raw.split(';').map(str::trim) {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key.trim().to_ascii_lowercase().as_str() {
            "filename*" => {
                let value = trim_header_value(value);
                let encoded = value.splitn(3, '\'').nth(2).unwrap_or(value);
                filename = percent_decode(encoded).or_else(|| Some(encoded.to_string()));
                break;
            }
            "filename" if filename.is_none() => {
                let value = trim_header_value(value);
                filename = percent_decode(value).or_else(|| Some(value.to_string()));
            }
            _ => {}
        }
    }
    filename.and_then(|name| sanitize_subscription_name(&strip_subscription_extension(&name)))
}

fn decode_base64_text(value: &str) -> Option<String> {
    let compact = value.split_whitespace().collect::<String>();
    for engine in [
        &general_purpose::STANDARD,
        &general_purpose::STANDARD_NO_PAD,
        &general_purpose::URL_SAFE,
        &general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(decoded) = engine.decode(compact.as_bytes()) {
            if let Ok(text) = String::from_utf8(decoded) {
                if let Some(name) = sanitize_subscription_name(&text) {
                    return Some(name);
                }
            }
        }
    }
    None
}

fn percent_decode(value: &str) -> Option<String> {
    let decoded = urlencoding::decode(value).ok()?;
    match decoded {
        Cow::Borrowed(_) => None,
        Cow::Owned(value) => Some(value),
    }
}

fn sanitize_subscription_name(value: &str) -> Option<String> {
    let normalized = trim_header_value(value)
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(value)
        .replace(SUB_DELIMITER, " ");
    let normalized = normalized
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>();
    let normalized = normalized.trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn strip_subscription_extension(value: &str) -> String {
    for suffix in [".yaml", ".yml", ".conf", ".txt"] {
        if value.to_ascii_lowercase().ends_with(suffix) {
            return value[..value.len() - suffix.len()].to_string();
        }
    }
    value.to_string()
}

fn trim_header_value(value: &str) -> &str {
    value.trim().trim_matches('"').trim_matches('\'').trim()
}

fn generated_subscription_name() -> String {
    let digits = now_iso()
        .chars()
        .filter(char::is_ascii_digit)
        .take(14)
        .collect::<String>();
    if digits.len() == 14 {
        format!("subscription-{digits}")
    } else {
        format!("subscription-{}", new_id("auto"))
    }
}

fn parse_subscription_userinfo(value: &HeaderValue, meta: &mut SubscriptionMeta) {
    let Ok(raw) = value.to_str() else {
        return;
    };
    for part in raw.split(';') {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        let parsed = value
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|value| i64::try_from(*value).is_ok());
        match key.trim().to_ascii_lowercase().as_str() {
            "upload" => meta.upload = parsed,
            "download" => meta.download = parsed,
            "total" => meta.total = parsed,
            "expire" => {
                meta.expire =
                    parsed.map(|ts| unix_seconds_to_display(ts).unwrap_or_else(|| ts.to_string()))
            }
            _ => {}
        }
    }
}

fn unix_seconds_to_display(value: u64) -> Option<String> {
    let datetime = time::OffsetDateTime::from_unix_timestamp(i64::try_from(value).ok()?).ok()?;
    let mut formatted = datetime
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now_iso());
    if formatted.len() >= 10 {
        formatted.truncate(10);
    }
    Some(formatted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_userinfo_drops_values_that_cannot_fit_in_sqlite() {
        let mut meta = SubscriptionMeta::default();
        let header = HeaderValue::from_str(&format!(
            "upload={}; download=42; total={}; expire={}",
            u64::MAX,
            u64::MAX,
            u64::MAX
        ))
        .expect("valid header");

        parse_subscription_userinfo(&header, &mut meta);

        assert_eq!(meta.upload, None);
        assert_eq!(meta.download, Some(42));
        assert_eq!(meta.total, None);
        assert_eq!(meta.expire, None);
    }

    #[test]
    fn discard_rule_filters_matching_node() {
        let rules = vec![FilterRule {
            id: "r1".into(),
            action: "discard".into(),
            match_type: "contains".into(),
            pattern: "官网".into(),
            values: Vec::new(),
            enabled: true,
        }];

        let (included, reason) = apply_filter_rules("机场官网", &rules, false);
        assert!(!included);
        assert!(reason.unwrap().contains("discard"));
    }

    #[test]
    fn keep_rules_make_non_matching_nodes_excluded() {
        let rules = vec![FilterRule {
            id: "r1".into(),
            action: "keep".into(),
            match_type: "contains".into(),
            pattern: "香港".into(),
            values: Vec::new(),
            enabled: true,
        }];

        assert!(apply_filter_rules("香港 01", &rules, true).0);
        assert!(!apply_filter_rules("日本 01", &rules, true).0);
    }

    #[test]
    fn in_rule_matches_exact_node_names() {
        let rules = vec![FilterRule {
            id: "r1".into(),
            action: "keep".into(),
            match_type: "in".into(),
            pattern: String::new(),
            values: vec!["香港 01".into(), "日本 01".into()],
            enabled: true,
        }];

        assert!(apply_filter_rules("香港 01", &rules, true).0);
        assert!(!apply_filter_rules("香港 02", &rules, true).0);
    }

    #[test]
    fn parses_base64_wrapped_yaml_subscription() {
        let yaml = r#"
proxies:
  - name: HK 01
    type: ss
proxy-groups:
  - name: Proxy
    type: select
    proxies:
      - HK 01
"#;
        let encoded = general_purpose::STANDARD.encode(yaml);
        let parsed = parse_subscription_document(&encoded).unwrap();

        assert_eq!(parsed.source_format, "base64-clash");
        assert!(parsed
            .value
            .as_mapping()
            .unwrap()
            .contains_key(Value::String("proxies".into())));
    }

    #[test]
    fn parses_base64_legacy_ss_subscription() {
        let ss_payload = general_purpose::STANDARD.encode("aes-128-gcm:pass@example.com:8388");
        let legacy = format!("ss://{ss_payload}#HK%2001");
        let encoded = general_purpose::STANDARD.encode(legacy);
        let parsed = parse_subscription_document(&encoded).unwrap();
        let proxies = parsed
            .value
            .as_mapping()
            .unwrap()
            .get(Value::String("proxies".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        let node = proxies[0].as_mapping().unwrap();

        assert_eq!(parsed.source_format, "base64-legacy");
        assert_eq!(
            node.get(Value::String("name".into()))
                .and_then(Value::as_str),
            Some("HK 01")
        );
        assert_eq!(
            node.get(Value::String("server".into()))
                .and_then(Value::as_str),
            Some("example.com")
        );
    }

    #[test]
    fn rejects_reachable_subscription_group_cycles_before_commit() {
        let yaml = serde_yaml::from_str::<Value>(
            r#"
proxies:
  - name: HK 01
    type: ss
    server: example.com
    port: 8388
    cipher: aes-128-gcm
    password: test
proxy-groups:
  - name: A
    type: select
    proxies: [B, HK 01]
  - name: B
    type: select
    proxies: [A]
"#,
        )
        .expect("parse cycle fixture");

        let error = parse_assets("sub_cycle", &yaml, &[])
            .expect_err("a reachable group cycle must be rejected before assets are saved");

        assert_eq!(error.code, "proxy_group_cycle");
    }

    #[test]
    fn remote_probe_groups_replace_loopback_and_link_local_urls() {
        let yaml = serde_yaml::from_str::<Value>(
            r#"
proxies:
  - { name: Node, type: ss }
proxy-groups:
  - name: Loopback Probe
    type: url-test
    proxies: [Node]
    url: http://127.0.0.1:8080/admin
    interval: 600
  - name: Metadata Probe
    type: fallback
    proxies: [Node]
    url: http://169.254.169.254/latest/meta-data
    interval: 600
"#,
        )
        .expect("parse URL fixture");

        let parsed = parse_assets("sub_probe_url", &yaml, &[]).expect("normalize probe groups");

        assert_eq!(parsed.groups.len(), 2);
        assert!(parsed
            .groups
            .iter()
            .all(|group| group.url.as_deref() == Some(DEFAULT_DELAY_TEST_URL)));
    }

    #[test]
    fn remote_probe_group_numbers_are_clamped_to_safe_ranges() {
        let yaml = serde_yaml::from_str::<Value>(
            r#"
proxies:
  - { name: Node, type: ss }
proxy-groups:
  - name: Fast Probe
    type: url-test
    proxies: [Node]
    interval: 1
    tolerance: 999999
  - name: Negative Probe
    type: fallback
    proxies: [Node]
    interval: -10
    tolerance: -100
  - name: Slow Probe
    type: load-balance
    proxies: [Node]
    interval: 9999999
"#,
        )
        .expect("parse numeric fixture");

        let parsed =
            parse_assets("sub_probe_numbers", &yaml, &[]).expect("normalize probe group numbers");
        let fast = parsed
            .groups
            .iter()
            .find(|group| group.display_name == "Fast Probe")
            .expect("fast group");
        let negative = parsed
            .groups
            .iter()
            .find(|group| group.display_name == "Negative Probe")
            .expect("negative group");
        let slow = parsed
            .groups
            .iter()
            .find(|group| group.display_name == "Slow Probe")
            .expect("slow group");

        assert_eq!(fast.interval, Some(MIN_ACTIVE_PROBE_INTERVAL_SECONDS));
        assert_eq!(fast.tolerance, Some(MAX_PROBE_TOLERANCE_MS));
        assert_eq!(negative.interval, Some(MIN_ACTIVE_PROBE_INTERVAL_SECONDS));
        assert_eq!(negative.tolerance, None);
        assert_eq!(slow.interval, Some(MAX_ACTIVE_PROBE_INTERVAL_SECONDS));
    }

    #[test]
    fn rejects_unsupported_remote_proxy_group_types() {
        let yaml = serde_yaml::from_str::<Value>(
            r#"
proxies:
  - { name: Node, type: ss }
proxy-groups:
  - name: Scripted
    type: relay
    proxies: [Node]
"#,
        )
        .expect("parse unsupported type fixture");

        let error = parse_assets("sub_unsupported_group", &yaml, &[])
            .expect_err("unsupported proxy group types must reject the refresh");

        assert_eq!(error.code, "subscription_unsupported_group_type");
    }

    #[test]
    fn automatic_remote_groups_drop_builtin_members() {
        let yaml = serde_yaml::from_str::<Value>(
            r#"
proxies:
  - { name: Node, type: ss }
proxy-groups:
  - name: Automatic
    type: url-test
    proxies: [DIRECT, REJECT, GLOBAL, PROXY, Node]
  - name: Manual
    type: select
    proxies: [DIRECT, REJECT, GLOBAL, PROXY, Node]
"#,
        )
        .expect("parse builtin member fixture");

        let parsed =
            parse_assets("sub_builtin_members", &yaml, &[]).expect("normalize group members");
        let automatic = parsed
            .groups
            .iter()
            .find(|group| group.display_name == "Automatic")
            .expect("automatic group");
        let manual = parsed
            .groups
            .iter()
            .find(|group| group.display_name == "Manual")
            .expect("manual group");

        assert_eq!(automatic.members, vec!["Node^_^sub_builtin_members"]);
        assert!(manual.members.iter().any(|member| member == "DIRECT"));
        assert!(manual.members.iter().any(|member| member == "PROXY"));
    }

    #[tokio::test]
    async fn invalid_proxy_type_is_rejected_before_old_assets_and_metadata_are_replaced() {
        let temp = TestDir::new("candidate-invalid-type");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app paths");
        let storage = Storage::connect(&paths).await.expect("connect storage");
        let subscription_id = "sub_candidate_invalid";
        storage
            .create_subscription(
                subscription_id,
                "Old Provider",
                "https://example.com/old.yaml",
                3600,
                true,
                &[],
            )
            .await
            .expect("create subscription");
        let old_node = candidate_node_item(
            subscription_id,
            "Old^_^sub_candidate_invalid",
            "Old",
            r#"{"name":"Old^_^sub_candidate_invalid","type":"ss","server":"old.example","port":443,"cipher":"aes-128-gcm","password":"old"}"#,
        );
        storage
            .replace_subscription_assets(
                subscription_id,
                std::slice::from_ref(&old_node),
                &[],
                candidate_commit("Old Provider", "old-hash"),
            )
            .await
            .expect("store old assets");

        let invalid_node = candidate_node_item(
            subscription_id,
            "Invalid^_^sub_candidate_invalid",
            "Invalid",
            r#"{"name":"Invalid^_^sub_candidate_invalid","type":"not-a-mihomo-proxy","server":"invalid.example","port":443}"#,
        );
        let syncer = SubscriptionSyncer::new(storage.clone(), paths);
        let error = syncer
            .validate_and_replace_assets(
                subscription_id,
                &[invalid_node],
                &[],
                candidate_commit("New Provider", "new-hash"),
                false,
            )
            .await
            .expect_err("invalid proxy type must fail candidate validation");

        assert_eq!(error.code, "subscription_candidate_invalid");
        let items = storage
            .proxy_items_for_runtime()
            .await
            .expect("list retained assets");
        assert!(items.iter().any(|item| item.name == old_node.name));
        assert!(items
            .iter()
            .all(|item| item.name != "Invalid^_^sub_candidate_invalid"));
        let subscription = storage
            .list_subscriptions()
            .await
            .expect("list subscriptions")
            .into_iter()
            .find(|subscription| subscription.id == subscription_id)
            .expect("subscription");
        assert_eq!(subscription.name, "Old Provider");
        assert_eq!(subscription.nodes, 1);
    }

    #[tokio::test]
    async fn missing_required_proxy_field_is_rejected_before_commit() {
        let temp = TestDir::new("candidate-missing-field");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app paths");
        let storage = Storage::connect(&paths).await.expect("connect storage");
        let subscription_id = "sub_candidate_missing";
        storage
            .create_subscription(
                subscription_id,
                "Provider",
                "https://example.com/profile.yaml",
                3600,
                true,
                &[],
            )
            .await
            .expect("create subscription");
        let missing_server = candidate_node_item(
            subscription_id,
            "Missing^_^sub_candidate_missing",
            "Missing",
            r#"{"name":"Missing^_^sub_candidate_missing","type":"ss","port":443,"cipher":"aes-128-gcm","password":"test"}"#,
        );
        let syncer = SubscriptionSyncer::new(storage.clone(), paths);

        let error = syncer
            .validate_and_replace_assets(
                subscription_id,
                &[missing_server],
                &[],
                candidate_commit("Changed Provider", "changed-hash"),
                false,
            )
            .await
            .expect_err("missing server must fail candidate validation");

        assert_eq!(error.code, "subscription_candidate_invalid");
        assert!(error.message.contains("has no server"));
        assert!(storage
            .proxy_items_for_runtime()
            .await
            .expect("list assets")
            .into_iter()
            .all(|item| item.subscription_id.as_deref() != Some(subscription_id)));
    }

    #[tokio::test]
    async fn filtered_invalid_node_does_not_block_a_valid_candidate_commit() {
        let temp = TestDir::new("candidate-filtered-invalid");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app paths");
        let storage = Storage::connect(&paths).await.expect("connect storage");
        let subscription_id = "sub_candidate_filtered";
        storage
            .create_subscription(
                subscription_id,
                "Provider",
                "https://example.com/profile.yaml",
                3600,
                true,
                &[],
            )
            .await
            .expect("create subscription");
        let valid_node = candidate_node_item(
            subscription_id,
            "Valid^_^sub_candidate_filtered",
            "Valid",
            r#"{"name":"Valid^_^sub_candidate_filtered","type":"ss","server":"valid.example","port":443,"cipher":"aes-128-gcm","password":"test"}"#,
        );
        let mut filtered_invalid = candidate_node_item(
            subscription_id,
            "Filtered^_^sub_candidate_filtered",
            "Filtered",
            r#"{"name":"Filtered^_^sub_candidate_filtered","type":"not-a-mihomo-proxy"}"#,
        );
        filtered_invalid.filtered_out = true;
        filtered_invalid.filter_reason = Some("discarded by test filter".into());
        let items = vec![valid_node.clone(), filtered_invalid.clone()];
        let candidate = crate::runtime::subscription_candidate_yaml(&items, &[])
            .expect("build filtered candidate");
        assert!(candidate.contains(&valid_node.name));
        assert!(!candidate.contains(&filtered_invalid.name));

        SubscriptionSyncer::new(storage.clone(), paths)
            .validate_and_replace_assets(
                subscription_id,
                &items,
                &[],
                candidate_commit("Provider", "filtered-hash"),
                false,
            )
            .await
            .expect("filtered invalid node must not block the valid candidate");

        let stored_items = storage
            .proxy_items_for_runtime()
            .await
            .expect("list stored assets");
        assert!(stored_items.iter().any(|item| item.name == valid_node.name));
        assert!(stored_items
            .iter()
            .any(|item| { item.name == filtered_invalid.name && item.filtered_out }));
        let subscription = storage
            .list_subscriptions()
            .await
            .expect("list subscriptions")
            .into_iter()
            .find(|subscription| subscription.id == subscription_id)
            .expect("subscription");
        assert_eq!(subscription.nodes, 1);
    }

    #[tokio::test]
    async fn candidate_config_file_is_private_and_removed_on_drop() {
        let temp = TestDir::new("candidate-file-cleanup");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app paths");
        let candidate = write_subscription_candidate(&paths, "password: sensitive\n")
            .await
            .expect("write candidate file");
        let candidate_path = candidate.path().to_path_buf();
        assert!(candidate_path.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&candidate_path)
                    .expect("candidate metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        drop(candidate);

        assert!(!candidate_path.exists());
    }

    #[tokio::test]
    async fn stopped_core_candidate_validation_does_not_execute_mihomo() {
        let temp = TestDir::new("candidate-stopped-core");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app paths");
        std::fs::write(paths.mihomo_binary(), b"not an executable")
            .expect("write invalid mihomo fixture");
        let candidate = r#"
proxies:
  - name: valid
    type: ss
    server: example.com
    port: 443
    cipher: aes-128-gcm
    password: test
"#;

        validate_subscription_candidate(&paths, candidate, false)
            .await
            .expect("stopped core must use structural validation only");

        assert_eq!(cleanup_stale_subscription_candidates(&paths), 0);
    }

    #[tokio::test]
    async fn refresh_lock_registry_rejects_missing_ids_and_prunes_finished_keys() {
        let temp = TestDir::new("subscription-refresh-locks");
        let paths = AppPaths::from_root(temp.path());
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        let syncer = SubscriptionSyncer::new(storage, paths);

        let missing = syncer
            .refresh("sub_missing", false)
            .await
            .expect_err("reject a missing subscription before allocating a lock");
        assert_eq!(missing.code, "subscription_not_found");
        assert!(syncer.refresh_locks.lock().await.is_empty());

        let finished = syncer.lock_refresh("sub_finished").await;
        drop(finished);
        let active = syncer.lock_refresh("sub_active").await;
        let locks = syncer.refresh_locks.lock().await;
        assert_eq!(locks.len(), 1);
        assert!(locks.contains_key("sub_active"));
        drop(locks);
        drop(active);
    }

    #[test]
    fn startup_cleanup_only_removes_regular_files_with_the_exact_candidate_name() {
        let temp = TestDir::new("stale-candidate-cleanup");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app paths");
        let stale = paths.profiles_dir.join(format!(
            "{SUBSCRIPTION_CANDIDATE_PREFIX}crashed{SUBSCRIPTION_CANDIDATE_SUFFIX}"
        ));
        let empty_name = paths.profiles_dir.join(".subscription-candidate-.yaml");
        let wrong_suffix = paths
            .profiles_dir
            .join(".subscription-candidate-crashed.yml");
        let extra_prefix = paths
            .profiles_dir
            .join("old.subscription-candidate-crashed.yaml");
        let matching_directory = paths
            .profiles_dir
            .join(".subscription-candidate-directory.yaml");
        for file in [&stale, &empty_name, &wrong_suffix, &extra_prefix] {
            std::fs::write(file, b"candidate").expect("write candidate fixture");
        }
        std::fs::create_dir(&matching_directory).expect("create matching directory");

        #[cfg(unix)]
        let (matching_symlink, symlink_target) = {
            use std::os::unix::fs::symlink;

            let target = paths.profiles_dir.join("candidate-target.yaml");
            let link = paths
                .profiles_dir
                .join(".subscription-candidate-symlink.yaml");
            std::fs::write(&target, b"keep me").expect("write symlink target");
            symlink(&target, &link).expect("create candidate symlink");
            (link, target)
        };

        assert_eq!(cleanup_stale_subscription_candidates(&paths), 1);

        assert!(!stale.exists());
        for preserved in [
            &empty_name,
            &wrong_suffix,
            &extra_prefix,
            &matching_directory,
        ] {
            assert!(preserved.exists(), "{}", preserved.display());
        }
        #[cfg(unix)]
        {
            assert!(matching_symlink.is_symlink());
            assert!(symlink_target.is_file());
        }
    }

    #[test]
    fn scoped_asset_names_are_stable_and_subscription_unique() {
        let mut used = HashSet::new();
        let first = scoped_name("HK 01", "sub_one", &mut used);
        let duplicate = scoped_name("HK 01", "sub_one", &mut used);
        let other_subscription = scoped_asset_name("HK 01", "sub_two");

        assert_eq!(first, "HK 01^_^sub_one");
        assert_eq!(duplicate, "HK 01^_^sub_one^_^2");
        assert_eq!(other_subscription, "HK 01^_^sub_two");
    }

    #[test]
    fn parses_plain_profile_title_header() {
        let value = HeaderValue::from_static("Remote Airport");

        assert_eq!(
            parse_profile_title(&value).as_deref(),
            Some("Remote Airport")
        );
    }

    #[test]
    fn parses_base64_profile_title_header() {
        let encoded = general_purpose::STANDARD.encode("Remote Airport");
        let value = HeaderValue::from_str(&format!("base64:{encoded}")).unwrap();

        assert_eq!(
            parse_profile_title(&value).as_deref(),
            Some("Remote Airport")
        );
    }

    #[test]
    fn parses_content_disposition_filename_header() {
        let value = HeaderValue::from_static("attachment; filename*=UTF-8''Remote%20Airport.yaml");

        assert_eq!(
            parse_content_disposition_filename(&value).as_deref(),
            Some("Remote Airport")
        );
    }

    fn candidate_node_item(
        subscription_id: &str,
        name: &str,
        display_name: &str,
        raw_json: &str,
    ) -> ProxyItemRecord {
        ProxyItemRecord {
            name: name.into(),
            kind: "node".into(),
            subscription_id: Some(subscription_id.into()),
            display_name: display_name.into(),
            source: "subscription".into(),
            builtin: false,
            source_name: Some("Provider".into()),
            protocol: Some("test".into()),
            country: None,
            group_type: None,
            raw_json: Some(raw_json.into()),
            content_hash: Some(content_hash(raw_json)),
            latency_ms: None,
            alive: true,
            filtered_out: false,
            filter_reason: None,
            delay_ms: None,
            tolerance_ms: None,
            url: None,
            interval_seconds: None,
            strategy_json: "{}".into(),
            position: 1024,
            enabled: true,
        }
    }

    fn candidate_commit(name: &str, raw_content_hash: &str) -> SubscriptionSyncCommit {
        SubscriptionSyncCommit {
            subscription_name: name.into(),
            node_count: 1,
            upload_bytes: None,
            download_bytes: None,
            total_bytes: None,
            expire_at: None,
            source_format: "clash".into(),
            raw_content_hash: raw_content_hash.into(),
        }
    }

    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("rweb-clash-subscription-{name}-{}", new_id("test")));
            std::fs::create_dir_all(&path).expect("create test directory");
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
