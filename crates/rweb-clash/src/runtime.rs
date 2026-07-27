use crate::error::AppError;
use crate::paths::{ensure_private_directory, restrict_sensitive_file_permissions, AppPaths};
use crate::rule::sanitize_rule_policy;
use crate::storage::{ProxyItemRecord, Storage};
use crate::types::{
    SystemConfig, BUILTIN_DIRECT, BUILTIN_GLOBAL, BUILTIN_PROXY, BUILTIN_REJECT,
    MIN_ACTIVE_PROBE_INTERVAL_SECONDS,
};
use crate::util::{new_id, valid_policy_target};
use serde_yaml::{Mapping, Value};
use std::collections::{HashMap, HashSet};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

struct RuntimeProxyPlan {
    item_count: usize,
    proxies: Vec<Value>,
    groups: Vec<Value>,
    available: HashSet<String>,
}

pub async fn compile_runtime_yaml(
    storage: &Storage,
    paths: &AppPaths,
    config: &SystemConfig,
) -> Result<std::path::PathBuf, AppError> {
    let proxy_plan = build_runtime_proxy_plan(storage, config.store_selected).await?;
    let rule_sets = storage.rule_sets_for_runtime().await?;
    let rules = storage.list_rules().await?;
    info!(
        proxy_items = proxy_plan.item_count,
        rule_sets = rule_sets.len(),
        rules = rules.len(),
        "building runtime config"
    );

    let proxy_count = proxy_plan.proxies.len();
    let group_count = proxy_plan.groups.len();

    let mut root = Mapping::new();
    insert(&mut root, "mixed-port", yaml_value(config.mixed_port));
    insert(&mut root, "allow-lan", yaml_value(config.allow_lan));
    insert(&mut root, "mode", Value::String(config.mode.clone()));
    insert(
        &mut root,
        "log-level",
        Value::String(config.log_level.clone()),
    );
    insert(&mut root, "ipv6", yaml_value(config.ipv6));
    if config.external_controller_enabled {
        insert(
            &mut root,
            "external-controller",
            Value::String(config.external_controller.clone()),
        );
        insert(&mut root, "secret", Value::String(config.secret.clone()));
    }
    insert(
        &mut root,
        "tcp-concurrent",
        yaml_value(config.tcp_concurrent),
    );
    insert(&mut root, "unified-delay", yaml_value(config.unified_delay));
    insert(&mut root, "profile", profile_mapping(config));
    if config.dns_enabled {
        insert(&mut root, "dns", dns_mapping(config));
    }
    if config.tun {
        insert(&mut root, "tun", tun_mapping());
    }
    insert(&mut root, "proxies", Value::Sequence(proxy_plan.proxies));
    insert(
        &mut root,
        "proxy-groups",
        Value::Sequence(proxy_plan.groups),
    );

    let (providers, available_rule_sets) = rule_provider_mapping(paths, &rule_sets);
    if !providers.is_empty() {
        insert(&mut root, "rule-providers", Value::Mapping(providers));
    }

    let mut rule_lines = Vec::new();
    for rule in rules.into_iter().filter(|rule| rule.enabled) {
        if rule.rule_type == "RULE-SET" && !available_rule_sets.contains(&rule.value) {
            warn!(
                rule_id = %rule.id,
                rule_set = %rule.value,
                "rule set has no validated local snapshot, terminating with fail-closed fallback"
            );
            rule_lines.push(Value::String(format!("MATCH,{BUILTIN_REJECT}")));
            continue;
        }
        let policy_is_valid = valid_policy_target(&rule.policy, &proxy_plan.available);
        let policy = sanitize_rule_policy(&rule.policy, &proxy_plan.available);
        if !policy_is_valid {
            warn!(
                rule_id = %rule.id,
                policy = %rule.policy,
                fallback_policy = %policy,
                "rule policy target is unavailable, using fail-closed fallback"
            );
        }
        let line = match rule.rule_type.as_str() {
            "MATCH" => format!("MATCH,{policy}"),
            "RULE-SET" => format!("RULE-SET,{},{}", rule.value, policy),
            _ => format!("{},{},{}", rule.rule_type, rule.value, policy),
        };
        rule_lines.push(Value::String(line));
    }
    if rule_lines.is_empty() {
        rule_lines.push(Value::String(format!("MATCH,{BUILTIN_DIRECT}")));
    }
    let rule_count = rule_lines.len();
    insert(&mut root, "rules", Value::Sequence(rule_lines));

    let yaml = serde_yaml::to_string(&Value::Mapping(root))?;
    write_runtime(paths, &yaml).await?;
    info!(
        runtime_yaml = %AppPaths::display(&paths.runtime_yaml),
        proxies = proxy_count,
        groups = group_count,
        rules = rule_count,
        "runtime config written"
    );
    Ok(paths.runtime_yaml.clone())
}

pub(crate) async fn available_policy_targets(
    storage: &Storage,
) -> Result<HashSet<String>, AppError> {
    Ok(build_runtime_proxy_plan(storage, false).await?.available)
}

async fn build_runtime_proxy_plan(
    storage: &Storage,
    restore_persisted_selections: bool,
) -> Result<RuntimeProxyPlan, AppError> {
    let items = storage.proxy_items_for_runtime().await?;
    let mut proxies = Vec::new();
    let mut available_nodes = HashSet::new();
    for item in items
        .iter()
        .filter(|item| item.kind == "node" && item.enabled && !item.filtered_out)
    {
        let Some(raw) = &item.raw_json else {
            warn!(node = %item.name, "dropping node with missing runtime JSON");
            continue;
        };
        match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(json_value) => {
                proxies.push(serde_yaml::to_value(json_value)?);
                available_nodes.insert(item.name.clone());
            }
            Err(error) => {
                warn!(node = %item.name, %error, "dropping node with invalid runtime JSON")
            }
        }
    }

    let (groups, available_groups) = build_groups(
        storage,
        &items,
        &available_nodes,
        restore_persisted_selections,
    )
    .await?;
    let available = available_nodes
        .into_iter()
        .chain(available_groups)
        .collect();
    Ok(RuntimeProxyPlan {
        item_count: items.len(),
        proxies,
        groups,
        available,
    })
}

async fn build_groups(
    storage: &Storage,
    items: &[ProxyItemRecord],
    available_nodes: &HashSet<String>,
    restore_persisted_selections: bool,
) -> Result<(Vec<Value>, HashSet<String>), AppError> {
    let mut member_map = HashMap::new();
    let nodes = items
        .iter()
        .filter(|item| item.kind == "node" && item.enabled && !item.filtered_out)
        .map(|item| crate::types::ProxyNodeResponse {
            name: item.name.clone(),
            display_name: item.display_name.clone(),
            protocol: item.protocol.clone().unwrap_or_else(|| "unknown".into()),
            latency: item.latency_ms.unwrap_or(0),
            country: item.country.clone(),
            subscription_id: item.subscription_id.clone(),
            subscription_name: item.source_name.clone(),
        })
        .collect::<Vec<_>>();
    for item in items
        .iter()
        .filter(|item| item.kind == "group" && item.enabled && !item.filtered_out)
    {
        let members = if item.source == "custom" {
            let filters = storage.group_filters(&item.name).await?;
            crate::proxy::calculate_members(&nodes, &filters)
        } else {
            storage.group_members(&item.name).await?
        };
        member_map.insert(item.name.clone(), members);
    }
    let mut builtin_members = nodes
        .iter()
        .map(|node| node.name.clone())
        .collect::<Vec<_>>();
    let candidate_groups = member_map
        .keys()
        .filter(|name| name.as_str() != BUILTIN_PROXY)
        .cloned()
        .collect::<Vec<_>>();
    for group_name in candidate_groups {
        if !group_depends_on(&member_map, &group_name, BUILTIN_PROXY, &mut HashSet::new()) {
            builtin_members.push(group_name);
        }
    }
    member_map.insert(BUILTIN_PROXY.into(), builtin_members);
    build_groups_from_members(
        items,
        member_map,
        available_nodes,
        restore_persisted_selections,
    )
}

fn group_depends_on(
    member_map: &HashMap<String, Vec<String>>,
    group_name: &str,
    target: &str,
    visiting: &mut HashSet<String>,
) -> bool {
    if group_name == target {
        return true;
    }
    if !visiting.insert(group_name.to_string()) {
        return false;
    }
    let depends = member_map.get(group_name).is_some_and(|members| {
        members.iter().any(|member| {
            member == target
                || (member_map.contains_key(member)
                    && group_depends_on(member_map, member, target, visiting))
        })
    });
    visiting.remove(group_name);
    depends
}

fn build_groups_from_members(
    items: &[ProxyItemRecord],
    mut member_map: HashMap<String, Vec<String>>,
    available_nodes: &HashSet<String>,
    restore_persisted_selections: bool,
) -> Result<(Vec<Value>, HashSet<String>), AppError> {
    let available_groups = resolve_available_groups(&member_map, available_nodes)?;

    let mut groups = Vec::new();
    for item in items
        .iter()
        .filter(|item| item.kind == "group" && item.enabled && !item.filtered_out)
    {
        if !available_groups.contains(&item.name) {
            warn!(group = %item.name, "dropping proxy group with no reachable members");
            continue;
        }
        let mut members = member_map.remove(&item.name).unwrap_or_default();
        members.retain(|member| {
            is_builtin_member(member)
                || available_nodes.contains(member)
                || available_groups.contains(member)
        });
        if item.name == BUILTIN_PROXY && members.is_empty() {
            members.push(BUILTIN_DIRECT.into());
        }
        if restore_persisted_selections && item.group_type.as_deref() == Some("select") {
            let selected = serde_json::from_str::<serde_json::Value>(&item.strategy_json)
                .ok()
                .and_then(|strategy| {
                    strategy
                        .get("now")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                });
            if let Some(index) = selected
                .as_deref()
                .and_then(|selected| members.iter().position(|member| member == selected))
            {
                let selected = members.remove(index);
                members.insert(0, selected);
            }
        }
        let mut mapping = Mapping::new();
        insert(&mut mapping, "name", Value::String(item.name.clone()));
        insert(
            &mut mapping,
            "type",
            Value::String(item.group_type.clone().unwrap_or_else(|| "select".into())),
        );
        insert(
            &mut mapping,
            "proxies",
            Value::Sequence(members.into_iter().map(Value::String).collect()),
        );
        if let Some(url) = &item.url {
            insert(&mut mapping, "url", Value::String(url.clone()));
        }
        if let Some(interval) = item.interval_seconds {
            let interval = if matches!(
                item.group_type.as_deref(),
                Some("url-test" | "fallback" | "load-balance")
            ) {
                interval.max(MIN_ACTIVE_PROBE_INTERVAL_SECONDS)
            } else {
                interval
            };
            insert(&mut mapping, "interval", yaml_value(interval));
        }
        if let Some(tolerance) = item.tolerance_ms {
            insert(&mut mapping, "tolerance", yaml_value(tolerance));
        }
        groups.push(Value::Mapping(mapping));
    }
    Ok((groups, available_groups))
}

pub(crate) fn available_policy_targets_from_assets(
    items: &[ProxyItemRecord],
    group_members: &HashMap<String, Vec<String>>,
) -> Result<HashSet<String>, AppError> {
    let available_nodes = items
        .iter()
        .filter(|item| {
            item.kind == "node"
                && item.enabled
                && !item.filtered_out
                && item
                    .raw_json
                    .as_deref()
                    .is_some_and(|raw| serde_json::from_str::<serde_json::Value>(raw).is_ok())
        })
        .map(|item| item.name.clone())
        .collect::<HashSet<_>>();
    let enabled_group_members = items
        .iter()
        .filter(|item| item.kind == "group" && item.enabled && !item.filtered_out)
        .map(|item| {
            (
                item.name.clone(),
                group_members.get(&item.name).cloned().unwrap_or_default(),
            )
        })
        .collect::<HashMap<_, _>>();
    let available_groups = resolve_available_groups(&enabled_group_members, &available_nodes)?;
    Ok(available_nodes
        .into_iter()
        .chain(available_groups)
        .collect())
}

pub(crate) fn subscription_candidate_yaml(
    items: &[ProxyItemRecord],
    group_members: &[(String, Vec<String>)],
) -> Result<String, AppError> {
    let mut proxies = Vec::new();
    let mut available_nodes = HashSet::new();
    for item in items
        .iter()
        .filter(|item| item.kind == "node" && item.enabled && !item.filtered_out)
    {
        let raw = item.raw_json.as_deref().ok_or_else(|| {
            AppError::bad_request(
                "subscription_candidate_invalid",
                format!(
                    "subscription node {} has no runtime configuration",
                    item.name
                ),
            )
        })?;
        let json_value = serde_json::from_str::<serde_json::Value>(raw).map_err(|error| {
            AppError::bad_request(
                "subscription_candidate_invalid",
                format!("subscription node {} has invalid JSON: {error}", item.name),
            )
        })?;
        proxies.push(serde_yaml::to_value(json_value)?);
        available_nodes.insert(item.name.clone());
    }

    let member_map = group_members.iter().cloned().collect::<HashMap<_, _>>();
    let (mut groups, _) = build_groups_from_members(items, member_map, &available_nodes, false)?;
    groups.insert(0, builtin_proxy_group());

    let mut root = Mapping::new();
    insert(&mut root, "mode", Value::String("rule".into()));
    insert(&mut root, "proxies", Value::Sequence(proxies));
    insert(&mut root, "proxy-groups", Value::Sequence(groups));
    insert(
        &mut root,
        "rules",
        Value::Sequence(vec![Value::String(format!("MATCH,{BUILTIN_DIRECT}"))]),
    );
    serde_yaml::to_string(&Value::Mapping(root)).map_err(AppError::from)
}

fn builtin_proxy_group() -> Value {
    let mut mapping = Mapping::new();
    insert(&mut mapping, "name", Value::String(BUILTIN_PROXY.into()));
    insert(&mut mapping, "type", Value::String("select".into()));
    insert(
        &mut mapping,
        "proxies",
        Value::Sequence(vec![Value::String(BUILTIN_DIRECT.into())]),
    );
    Value::Mapping(mapping)
}

pub(crate) fn resolve_available_groups(
    member_map: &HashMap<String, Vec<String>>,
    available_nodes: &HashSet<String>,
) -> Result<HashSet<String>, AppError> {
    let mut available_groups = HashSet::new();
    loop {
        let mut changed = false;
        for group_name in member_map.keys() {
            if available_groups.contains(group_name) {
                continue;
            }
            let has_available_member =
                member_map
                    .get(group_name)
                    .into_iter()
                    .flatten()
                    .any(|member| {
                        is_builtin_member(member)
                            || available_nodes.contains(member)
                            || available_groups.contains(member)
                    });
            if group_name == BUILTIN_PROXY || has_available_member {
                changed |= available_groups.insert(group_name.clone());
            }
        }
        if !changed {
            break;
        }
    }
    validate_group_graph(member_map, &available_groups)?;
    Ok(available_groups)
}

fn is_builtin_member(member: &str) -> bool {
    matches!(
        member,
        BUILTIN_DIRECT | BUILTIN_REJECT | BUILTIN_GLOBAL | BUILTIN_PROXY
    )
}

fn validate_group_graph(
    member_map: &HashMap<String, Vec<String>>,
    available_groups: &HashSet<String>,
) -> Result<(), AppError> {
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for name in available_groups {
        dfs_group(
            name,
            member_map,
            available_groups,
            &mut visiting,
            &mut visited,
        )?;
    }
    Ok(())
}

fn dfs_group(
    name: &str,
    member_map: &HashMap<String, Vec<String>>,
    available_groups: &HashSet<String>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
) -> Result<(), AppError> {
    if visited.contains(name) {
        return Ok(());
    }
    if !visiting.insert(name.to_string()) {
        return Err(AppError::bad_request(
            "proxy_group_cycle",
            format!("proxy group cycle detected at {name}"),
        ));
    }
    for dep in member_map
        .get(name)
        .into_iter()
        .flatten()
        .filter(|member| available_groups.contains(*member))
    {
        dfs_group(dep, member_map, available_groups, visiting, visited)?;
    }
    visiting.remove(name);
    visited.insert(name.to_string());
    Ok(())
}

fn dns_mapping(config: &SystemConfig) -> Value {
    let mut dns = Mapping::new();
    insert(&mut dns, "enable", yaml_value(true));
    insert(&mut dns, "listen", Value::String("127.0.0.1:1053".into()));
    insert(&mut dns, "ipv6", yaml_value(config.ipv6));
    insert(
        &mut dns,
        "enhanced-mode",
        Value::String(config.dns_mode.clone()),
    );
    insert(
        &mut dns,
        "nameserver",
        yaml_string_sequence(&config.dns_nameservers),
    );
    if !config.dns_fallback.is_empty() {
        insert(
            &mut dns,
            "fallback",
            yaml_string_sequence(&config.dns_fallback),
        );
    }
    if !config.dns_fake_ip_filter.is_empty() {
        insert(
            &mut dns,
            "fake-ip-filter",
            yaml_string_sequence(&config.dns_fake_ip_filter),
        );
    }
    if !config.dns_nameserver_policy.is_empty() {
        insert(
            &mut dns,
            "nameserver-policy",
            string_list_mapping(&config.dns_nameserver_policy),
        );
    }
    if !config.dns_hosts.is_empty() {
        insert(&mut dns, "hosts", string_list_mapping(&config.dns_hosts));
    }
    Value::Mapping(dns)
}

fn yaml_string_sequence(values: &[String]) -> Value {
    Value::Sequence(values.iter().cloned().map(Value::String).collect())
}

fn string_list_mapping(values: &std::collections::BTreeMap<String, Vec<String>>) -> Value {
    let mut mapping = Mapping::new();
    for (key, entries) in values {
        let value = if entries.len() == 1 {
            Value::String(entries[0].clone())
        } else {
            yaml_string_sequence(entries)
        };
        mapping.insert(Value::String(key.clone()), value);
    }
    Value::Mapping(mapping)
}

fn profile_mapping(config: &SystemConfig) -> Value {
    let mut profile = Mapping::new();
    insert(
        &mut profile,
        "store-selected",
        yaml_value(config.store_selected),
    );
    Value::Mapping(profile)
}

fn tun_mapping() -> Value {
    let mut tun = Mapping::new();
    insert(&mut tun, "enable", yaml_value(true));
    insert(&mut tun, "stack", Value::String("system".into()));
    insert(&mut tun, "auto-route", yaml_value(true));
    insert(&mut tun, "auto-detect-interface", yaml_value(true));
    Value::Mapping(tun)
}

fn rule_provider_mapping(
    paths: &AppPaths,
    rule_sets: &[crate::storage::RuleSetRecord],
) -> (Mapping, HashSet<String>) {
    let mut providers = Mapping::new();
    let mut available = HashSet::new();
    for rule_set in rule_sets {
        let Some(local_path) = rule_set.local_path.as_deref() else {
            warn!(rule_set = %rule_set.name, "rule set has no validated local snapshot");
            continue;
        };
        let local_path = paths.resolve_local_path(local_path);
        if !local_path.is_file() {
            warn!(
                rule_set = %rule_set.name,
                path = %AppPaths::display(&local_path),
                "rule set local snapshot is missing"
            );
            continue;
        }
        let mut provider = Mapping::new();
        insert(&mut provider, "type", Value::String("file".into()));
        insert(
            &mut provider,
            "behavior",
            Value::String(
                rule_set
                    .behavior
                    .clone()
                    .unwrap_or_else(|| "classical".into()),
            ),
        );
        insert(
            &mut provider,
            "format",
            Value::String(rule_set.format.clone()),
        );
        insert(
            &mut provider,
            "path",
            Value::String(AppPaths::display(&local_path)),
        );
        providers.insert(
            Value::String(rule_set.name.clone()),
            Value::Mapping(provider),
        );
        available.insert(rule_set.name.clone());
    }
    (providers, available)
}

async fn write_runtime(paths: &AppPaths, yaml: &str) -> Result<(), AppError> {
    ensure_private_directory(&paths.profiles_dir)?;
    restrict_sensitive_file_permissions(&paths.runtime_yaml)?;
    let tmp = paths
        .runtime_yaml
        .with_extension(format!("yaml.{}.tmp", new_id("runtime")));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(crate::paths::PRIVATE_FILE_MODE);
    let mut file = options.open(&tmp).await?;
    file.write_all(yaml.as_bytes()).await?;
    file.flush().await?;
    drop(file);
    restrict_sensitive_file_permissions(&tmp)?;
    if let Err(error) = replace_runtime_file(&tmp, &paths.runtime_yaml).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(AppError::from(error));
    }
    restrict_sensitive_file_permissions(&paths.runtime_yaml)?;
    Ok(())
}

#[cfg(not(windows))]
async fn replace_runtime_file(
    tmp: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    tokio::fs::rename(tmp, target).await
}

#[cfg(windows)]
async fn replace_runtime_file(
    tmp: &std::path::Path,
    target: &std::path::Path,
) -> std::io::Result<()> {
    let backup = target.with_extension("yaml.bak");
    if backup.exists() {
        tokio::fs::remove_file(&backup).await?;
    }
    let had_target = target.exists();
    if had_target {
        tokio::fs::rename(target, &backup).await?;
    }
    if let Err(error) = tokio::fs::rename(tmp, target).await {
        if had_target {
            let _ = tokio::fs::rename(&backup, target).await;
        }
        return Err(error);
    }
    if had_target {
        let _ = tokio::fs::remove_file(backup).await;
    }
    Ok(())
}

fn insert(mapping: &mut Mapping, key: &str, value: Value) {
    mapping.insert(Value::String(key.to_string()), value);
}

fn yaml_value<T: serde::Serialize>(value: T) -> Value {
    serde_yaml::to_value(value).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_listener_stays_loopback_when_lan_proxying_is_enabled() {
        let config = SystemConfig {
            allow_lan: true,
            ..SystemConfig::default()
        };

        let dns = dns_mapping(&config);
        let listen = dns
            .as_mapping()
            .and_then(|mapping| mapping.get(Value::String("listen".into())))
            .and_then(Value::as_str);

        assert_eq!(listen, Some("127.0.0.1:1053"));
    }

    #[test]
    fn profile_respects_the_store_selected_setting() {
        for expected in [false, true] {
            let config = SystemConfig {
                store_selected: expected,
                ..SystemConfig::default()
            };
            let actual = profile_mapping(&config)
                .as_mapping()
                .and_then(|mapping| mapping.get(Value::String("store-selected".into())))
                .and_then(Value::as_bool);

            assert_eq!(actual, Some(expected));
        }
    }

    #[test]
    fn select_groups_only_restore_persisted_selection_when_enabled() {
        let group = ProxyItemRecord {
            name: "Strategy".into(),
            kind: "group".into(),
            subscription_id: None,
            display_name: "Strategy".into(),
            source: "custom".into(),
            builtin: false,
            source_name: None,
            protocol: None,
            country: None,
            group_type: Some("select".into()),
            raw_json: None,
            content_hash: None,
            latency_ms: None,
            alive: true,
            filtered_out: false,
            filter_reason: None,
            delay_ms: None,
            tolerance_ms: None,
            url: None,
            interval_seconds: None,
            strategy_json: serde_json::json!({ "now": "Node B" }).to_string(),
            position: 1024,
            enabled: true,
        };
        let member_map =
            HashMap::from([("Strategy".into(), vec!["Node A".into(), "Node B".into()])]);
        let available_nodes = HashSet::from(["Node A".into(), "Node B".into()]);

        for (restore, expected) in [
            (false, vec!["Node A", "Node B"]),
            (true, vec!["Node B", "Node A"]),
        ] {
            let (groups, _) = build_groups_from_members(
                std::slice::from_ref(&group),
                member_map.clone(),
                &available_nodes,
                restore,
            )
            .expect("build select group");
            let proxies = groups[0]
                .as_mapping()
                .and_then(|mapping| mapping.get(Value::String("proxies".into())))
                .and_then(Value::as_sequence)
                .expect("proxy members")
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>();
            assert_eq!(proxies, expected);
        }
    }

    #[test]
    fn active_probe_groups_apply_the_runtime_interval_floor() {
        let group = ProxyItemRecord {
            name: "Automatic".into(),
            kind: "group".into(),
            subscription_id: None,
            display_name: "Automatic".into(),
            source: "custom".into(),
            builtin: false,
            source_name: None,
            protocol: None,
            country: None,
            group_type: Some("url-test".into()),
            raw_json: None,
            content_hash: None,
            latency_ms: None,
            alive: true,
            filtered_out: false,
            filter_reason: None,
            delay_ms: None,
            tolerance_ms: Some(50),
            url: Some(crate::types::DEFAULT_DELAY_TEST_URL.into()),
            interval_seconds: Some(300),
            strategy_json: "{}".into(),
            position: 1024,
            enabled: true,
        };
        let member_map = HashMap::from([("Automatic".into(), vec!["Node A".into()])]);
        let available_nodes = HashSet::from(["Node A".into()]);

        let (groups, _) = build_groups_from_members(&[group], member_map, &available_nodes, false)
            .expect("build active probe group");
        let interval = groups[0]
            .as_mapping()
            .and_then(|mapping| mapping.get(Value::String("interval".into())))
            .and_then(Value::as_i64);

        assert_eq!(interval, Some(MIN_ACTIVE_PROBE_INTERVAL_SECONDS));
    }

    #[tokio::test]
    async fn runtime_write_replaces_existing_file_without_leaving_staging_files() {
        let temp = TestDir::new("runtime-replace");
        let paths = AppPaths::from_root(temp.path());

        write_runtime(&paths, "first\n")
            .await
            .expect("write first runtime");
        write_runtime(&paths, "second\n")
            .await
            .expect("replace runtime");

        assert_eq!(
            tokio::fs::read_to_string(&paths.runtime_yaml)
                .await
                .expect("read runtime"),
            "second\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            assert_eq!(
                std::fs::metadata(&paths.runtime_yaml)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let mut entries = tokio::fs::read_dir(&paths.profiles_dir)
            .await
            .expect("read profiles directory");
        while let Some(entry) = entries.next_entry().await.expect("read directory entry") {
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(!name.ends_with(".tmp"), "staging file remained: {name}");
            assert!(!name.ends_with(".bak"), "backup file remained: {name}");
        }
    }

    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("rweb-clash-{name}-{}", new_id("test")));
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
