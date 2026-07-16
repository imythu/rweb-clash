use crate::error::AppError;
use crate::paths::AppPaths;
use crate::remote;
use crate::runtime::available_policy_targets;
use crate::storage::Storage;
use crate::types::{
    RuleInput, RuleResponse, RuleSetInput, RuleSetResponse, RuleTestRequest, RuleTestResponse,
    BUILTIN_DIRECT, BUILTIN_GLOBAL, BUILTIN_PROXY, BUILTIN_REJECT,
};
use crate::util::{content_hash, new_id, validate_url};
use ipnet::IpNet;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

const MAX_RULE_SET_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleMatchOutcome {
    Matched,
    NotMatched,
    MissingRuleSetSnapshot,
}

impl From<bool> for RuleMatchOutcome {
    fn from(matched: bool) -> Self {
        if matched {
            Self::Matched
        } else {
            Self::NotMatched
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuleService {
    storage: Storage,
    paths: AppPaths,
    refresh_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl RuleService {
    pub fn new(storage: Storage, paths: AppPaths) -> Self {
        Self {
            storage,
            paths,
            refresh_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn create_rule(&self, input: RuleInput) -> Result<RuleResponse, AppError> {
        validate_rule_input(&input)?;
        let value = normalized_rule_value(&input).to_string();
        info!(
            rule_type = %input.rule_type,
            value = %value,
            policy = %input.policy,
            enabled = input.enabled.unwrap_or(true),
            "storing routing rule"
        );
        self.storage
            .upsert_rule(
                input.id,
                &input.rule_type,
                &value,
                &input.policy,
                input.desc.as_deref(),
                input.enabled.unwrap_or(true),
            )
            .await
    }

    pub async fn update_rule(&self, id: &str, input: RuleInput) -> Result<RuleResponse, AppError> {
        validate_rule_input(&input)?;
        info!(
            rule_id = %id,
            rule_type = %input.rule_type,
            policy = %input.policy,
            enabled = input.enabled.unwrap_or(true),
            "storing routing rule update"
        );
        self.storage
            .update_rule(
                id,
                &input.rule_type,
                normalized_rule_value(&input),
                &input.policy,
                input.desc.as_deref(),
                input.enabled.unwrap_or(true),
            )
            .await
    }

    pub async fn test_rule(&self, request: RuleTestRequest) -> Result<RuleTestResponse, AppError> {
        if request.target.trim().is_empty() {
            return Err(AppError::bad_request(
                "rule_test_invalid_target",
                "target cannot be empty",
            ));
        }
        let rules = self.storage.list_rules().await?;
        let rule_sets = self.storage.rule_sets_for_runtime().await?;
        let available = available_policy_targets(&self.storage).await?;
        let target = request.target.trim();
        for rule in rules.iter().filter(|rule| rule.enabled) {
            match self.rule_matches(rule, target, &rule_sets).await? {
                RuleMatchOutcome::Matched => {
                    let final_proxy = sanitize_rule_policy(&rule.policy, &available);
                    info!(
                        target = %target,
                        rule_id = %rule.id,
                        policy = %rule.policy,
                        final_proxy = %final_proxy,
                        "rule test matched"
                    );
                    return Ok(RuleTestResponse {
                        hit_rule: rule.clone(),
                        final_proxy,
                    });
                }
                RuleMatchOutcome::MissingRuleSetSnapshot => {
                    warn!(
                        target = %target,
                        rule_id = %rule.id,
                        rule_set = %rule.value,
                        "rule test encountered a missing rule-set snapshot, failing closed"
                    );
                    return Ok(RuleTestResponse {
                        hit_rule: rule.clone(),
                        final_proxy: BUILTIN_REJECT.into(),
                    });
                }
                RuleMatchOutcome::NotMatched => {}
            }
        }
        let fallback = RuleResponse {
            id: "implicit_match".into(),
            rule_type: "MATCH".into(),
            value: "ANY".into(),
            policy: BUILTIN_DIRECT.into(),
            position: i64::MAX,
            source: "system".into(),
            enabled: true,
            desc: Some("Implicit fallback".into()),
        };
        info!(target = %target, "rule test fell back to implicit MATCH");
        Ok(RuleTestResponse {
            hit_rule: fallback,
            final_proxy: BUILTIN_DIRECT.into(),
        })
    }

    pub async fn create_rule_set(&self, input: RuleSetInput) -> Result<RuleSetResponse, AppError> {
        if input.name.trim().is_empty() {
            return Err(AppError::bad_request(
                "ruleset_invalid",
                "rule set name cannot be empty",
            ));
        }
        if !validate_url(&input.url) {
            return Err(AppError::bad_request(
                "ruleset_invalid_url",
                "rule set url must start with http:// or https://",
            ));
        }
        let id = new_id("rs");
        info!(
            rule_set_id = %id,
            name = %input.name.trim(),
            format = %input.format.as_deref().unwrap_or("text"),
            interval_seconds = input.interval_seconds(),
            url_host = %url_host_label(&input.url),
            "creating rule set"
        );
        self.storage
            .create_rule_set(
                &id,
                input.name.trim(),
                input.url.trim(),
                input.interval_seconds(),
                input.behavior.as_deref(),
                input.format.as_deref().unwrap_or("text"),
            )
            .await?;
        if let Err(fetch_error) = self.refresh_rule_set(&id).await {
            warn!(
                rule_set_id = %id,
                error = %fetch_error,
                "initial rule set fetch failed, rolling back record"
            );
            if let Err(rollback_error) = self.rollback_rule_set_creation(&id).await {
                return Err(AppError::internal(format!(
                    "initial rule set fetch failed ({fetch_error}); rollback failed: {rollback_error}"
                )));
            }
            return Err(fetch_error);
        }
        let all = self.storage.list_rule_sets().await?;
        all.into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| AppError::internal("created rule set disappeared"))
    }

    async fn rollback_rule_set_creation(&self, id: &str) -> Result<(), AppError> {
        self.storage.delete_rule_set(id).await?;
        let _ = tokio::fs::remove_file(self.paths.rule_sets_dir.join(format!("{id}.list"))).await;
        let _ =
            tokio::fs::remove_file(self.paths.rule_sets_dir.join(format!("{id}.list.tmp"))).await;
        Ok(())
    }

    pub async fn refresh_rule_set(&self, id: &str) -> Result<(), AppError> {
        let refresh_lock = {
            let mut locks = self.refresh_locks.lock().await;
            locks
                .entry(id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _refresh_guard = refresh_lock.lock().await;
        let Some(rule_set) = self
            .storage
            .rule_sets_for_runtime()
            .await?
            .into_iter()
            .find(|rule_set| rule_set.id == id)
        else {
            warn!(rule_set_id = %id, "rule set refresh requested for missing rule set");
            return Err(AppError::not_found(
                "ruleset_not_found",
                format!("rule set {id} not found"),
            ));
        };
        info!(
            rule_set_id = %id,
            name = %rule_set.name,
            url_host = %url_host_label(&rule_set.url),
            "fetching rule set"
        );
        let response = remote::get_text(
            &rule_set.url,
            None,
            MAX_RULE_SET_BYTES,
            "ruleset_fetch_failed",
        )
        .await?;
        let status = response.status;
        let content = response.body;
        let (content, rule_count, detected_format) = tokio::task::spawn_blocking(move || {
            let rule_count = count_rules(&content);
            let detected_format = detect_rule_set_format(&content);
            (content, rule_count, detected_format)
        })
        .await
        .map_err(|error| AppError::internal(format!("rule-set analyzer task failed: {error}")))?;
        info!(
            rule_set_id = %id,
            status = status.as_u16(),
            bytes = content.len(),
            rules = rule_count,
            format = %detected_format,
            "rule set fetched"
        );
        let local = self.paths.rule_sets_dir.join(format!("{id}.list"));
        let tmp = self
            .paths
            .rule_sets_dir
            .join(format!("{id}.{}.tmp", new_id("download")));
        tokio::fs::write(&tmp, &content).await?;
        if let Err(err) = replace_downloaded_file(&tmp, &local).await {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(err);
        }
        let relative = self.paths.rule_set_relative_path(id);
        self.storage
            .update_rule_set_refresh(
                id,
                &relative,
                content.len() as u64,
                rule_count,
                &content_hash(content.as_bytes()),
                detected_format,
                None,
            )
            .await?;
        info!(
            rule_set_id = %id,
            local_path = %relative,
            format = %detected_format,
            "rule set saved"
        );
        Ok(())
    }

    async fn rule_matches(
        &self,
        rule: &RuleResponse,
        target: &str,
        rule_sets: &[crate::storage::RuleSetRecord],
    ) -> Result<RuleMatchOutcome, AppError> {
        let target_lower = target.to_ascii_lowercase();
        let value_lower = rule.value.to_ascii_lowercase();
        match rule.rule_type.as_str() {
            "DOMAIN-SUFFIX" => Ok((target_lower == value_lower
                || target_lower.ends_with(&format!(".{value_lower}")))
            .into()),
            "DOMAIN" => Ok((target_lower == value_lower).into()),
            "DOMAIN-KEYWORD" => Ok(target_lower.contains(&value_lower).into()),
            "IP-CIDR" | "IP-CIDR6" => Ok(match_ip_cidr_for_test(target, &rule.value)?.into()),
            "GEOIP" => Err(AppError::bad_request(
                "rule_test_unsupported",
                "GEOIP rules require Mihomo's GeoIP database and cannot be evaluated locally",
            )),
            "MATCH" => Ok(RuleMatchOutcome::Matched),
            "RULE-SET" => {
                let Some(rule_set) = rule_sets.iter().find(|item| item.name == rule.value) else {
                    return Ok(RuleMatchOutcome::MissingRuleSetSnapshot);
                };
                let Some(local_path) = &rule_set.local_path else {
                    return Ok(RuleMatchOutcome::MissingRuleSetSnapshot);
                };
                let path = self.paths.resolve_local_path(local_path);
                let Ok(content) = tokio::fs::read_to_string(path).await else {
                    return Ok(RuleMatchOutcome::MissingRuleSetSnapshot);
                };
                let target = target.to_string();
                let behavior = rule_set
                    .behavior
                    .clone()
                    .unwrap_or_else(|| "classical".into());
                let matched = tokio::task::spawn_blocking(move || {
                    rule_set_contains(&content, &target, &behavior)
                })
                .await
                .map_err(|error| {
                    AppError::internal(format!("rule-set matcher task failed: {error}"))
                })??;
                Ok(matched.into())
            }
            _ => Ok(RuleMatchOutcome::NotMatched),
        }
    }
}

fn url_host_label(value: &str) -> String {
    reqwest::Url::parse(value)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into())
}

#[cfg(not(target_os = "windows"))]
async fn replace_downloaded_file(
    temporary: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), AppError> {
    tokio::fs::rename(temporary, destination).await?;
    Ok(())
}

#[cfg(target_os = "windows")]
async fn replace_downloaded_file(
    temporary: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), AppError> {
    if !destination.exists() {
        tokio::fs::rename(temporary, destination).await?;
        return Ok(());
    }

    let backup = destination.with_extension(format!("{}.bak", new_id("replace")));
    tokio::fs::rename(destination, &backup).await?;
    if let Err(replace_error) = tokio::fs::rename(temporary, destination).await {
        if let Err(restore_error) = tokio::fs::rename(&backup, destination).await {
            return Err(AppError::internal(format!(
                "rule set replace failed ({replace_error}); restoring the previous file failed ({restore_error}); backup retained at {}",
                AppPaths::display(&backup)
            )));
        }
        return Err(AppError::from(replace_error));
    }
    let _ = tokio::fs::remove_file(backup).await;
    Ok(())
}

pub fn validate_rule_input(input: &RuleInput) -> Result<(), AppError> {
    if !matches!(
        input.rule_type.as_str(),
        "RULE-SET"
            | "DOMAIN-SUFFIX"
            | "DOMAIN"
            | "DOMAIN-KEYWORD"
            | "IP-CIDR"
            | "IP-CIDR6"
            | "GEOIP"
            | "MATCH"
    ) {
        return Err(AppError::bad_request(
            "rule_invalid",
            format!("unsupported rule type {}", input.rule_type),
        ));
    }
    if input.rule_type != "MATCH" && input.value.trim().is_empty() {
        return Err(AppError::bad_request(
            "rule_invalid",
            "rule value cannot be empty",
        ));
    }
    if matches!(input.rule_type.as_str(), "IP-CIDR" | "IP-CIDR6")
        && input.value.trim().parse::<IpNet>().is_err()
    {
        return Err(AppError::bad_request(
            "rule_invalid",
            format!("invalid CIDR network {}", input.value),
        ));
    }
    if input.policy.trim().is_empty() {
        return Err(AppError::bad_request(
            "rule_invalid",
            "rule policy cannot be empty",
        ));
    }
    Ok(())
}

pub fn sanitize_rule_policy(policy: &str, available: &HashSet<String>) -> String {
    if matches!(
        policy,
        BUILTIN_DIRECT | BUILTIN_REJECT | BUILTIN_GLOBAL | BUILTIN_PROXY
    ) || available.contains(policy)
    {
        policy.to_string()
    } else {
        BUILTIN_REJECT.into()
    }
}

fn normalized_rule_value(input: &RuleInput) -> &str {
    if input.rule_type == "MATCH" {
        "ANY"
    } else {
        input.value.trim()
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

fn rule_set_contains(content: &str, target: &str, behavior: &str) -> Result<bool, AppError> {
    let target_lower = target.to_ascii_lowercase();
    for line in content.lines().filter_map(normalize_rule_set_line) {
        let lower = line.to_ascii_lowercase();
        if lower.contains(',') {
            let parts = lower.split(',').map(str::trim).collect::<Vec<_>>();
            if parts.len() >= 2 {
                let rule = RuleResponse {
                    id: String::new(),
                    rule_type: parts[0].to_ascii_uppercase(),
                    value: parts[1].to_string(),
                    policy: BUILTIN_DIRECT.into(),
                    position: 0,
                    source: "ruleset".into(),
                    enabled: true,
                    desc: None,
                };
                if matches_inline_rule(&rule, &target_lower)? {
                    return Ok(true);
                }
            }
            continue;
        }

        if behavior.eq_ignore_ascii_case("ipcidr") {
            if match_ip_cidr_for_test(target, line)? {
                return Ok(true);
            }
        } else {
            let domain = lower.trim_start_matches('+').trim_start_matches('.');
            if target_lower == domain || target_lower.ends_with(&format!(".{domain}")) {
                return Ok(true);
            }
        }
    }
    Ok(false)
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

fn matches_inline_rule(rule: &RuleResponse, target_lower: &str) -> Result<bool, AppError> {
    let value = rule.value.to_ascii_lowercase();
    Ok(match rule.rule_type.as_str() {
        "DOMAIN-SUFFIX" => target_lower == value || target_lower.ends_with(&format!(".{value}")),
        "DOMAIN" => target_lower == value,
        "DOMAIN-KEYWORD" => target_lower.contains(&value),
        "IP-CIDR" | "IP-CIDR6" => match_ip_cidr_for_test(target_lower, &value)?,
        "GEOIP" => {
            return Err(AppError::bad_request(
                "rule_test_unsupported",
                "a classical rule set contains GEOIP rules that cannot be evaluated locally",
            ));
        }
        "MATCH" => true,
        _ => false,
    })
}

#[cfg(test)]
fn ip_matches_cidr(target: &str, cidr: &str) -> bool {
    let Ok(address) = target.trim().parse::<std::net::IpAddr>() else {
        return false;
    };
    cidr.trim()
        .parse::<IpNet>()
        .is_ok_and(|network| network.contains(&address))
}

fn match_ip_cidr_for_test(target: &str, cidr: &str) -> Result<bool, AppError> {
    let address = target.trim().parse::<std::net::IpAddr>().map_err(|_| {
        AppError::bad_request(
            "rule_test_requires_ip",
            "IP-CIDR rules require a literal IPv4 or IPv6 target for deterministic local testing",
        )
    })?;
    let network = cidr.trim().parse::<IpNet>().map_err(|error| {
        AppError::bad_request(
            "rule_test_invalid_cidr",
            format!("rule contains an invalid CIDR network: {error}"),
        )
    })?;
    Ok(network.contains(&address))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_set_contains_domain_suffix_lines() {
        let content = r#"
# comment
.google.com
DOMAIN-KEYWORD,netflix
"#;

        assert!(rule_set_contains(content, "mail.google.com", "domain").unwrap());
        assert!(rule_set_contains(content, "www.netflix.com", "classical").unwrap());
        assert!(!rule_set_contains(content, "example.org", "classical").unwrap());
    }

    #[test]
    fn rule_set_contains_yaml_payload_lines() {
        let content = r#"
payload:
  - '+.google.com'
  - DOMAIN-KEYWORD,netflix
"#;

        assert_eq!(count_rules(content), 2);
        assert!(rule_set_contains(content, "mail.google.com", "domain").unwrap());
        assert!(rule_set_contains(content, "www.netflix.com", "classical").unwrap());
        assert!(!rule_set_contains(content, "example.org", "classical").unwrap());
    }

    #[test]
    fn cidr_matching_supports_ipv4_and_ipv6_prefixes() {
        assert!(ip_matches_cidr("10.24.1.8", "10.0.0.0/8"));
        assert!(ip_matches_cidr("192.168.42.1", "192.168.0.0/16"));
        assert!(ip_matches_cidr("2001:db8::42", "2001:db8::/32"));
        assert!(!ip_matches_cidr("11.0.0.1", "10.0.0.0/8"));
        assert!(!ip_matches_cidr("example.com", "10.0.0.0/8"));
    }

    #[test]
    fn cidr_rules_validate_networks_and_require_ip_test_targets() {
        let input = RuleInput {
            rule_type: "IP-CIDR".into(),
            value: "10.0.0.0/99".into(),
            policy: BUILTIN_DIRECT.into(),
            ..RuleInput::default()
        };
        assert!(validate_rule_input(&input).is_err());
        assert!(match_ip_cidr_for_test("example.com", "10.0.0.0/8").is_err());
    }

    #[test]
    fn ipcidr_rule_sets_match_plain_and_classical_entries() {
        assert!(rule_set_contains("10.0.0.0/8", "10.2.3.4", "ipcidr").unwrap());
        assert!(rule_set_contains("IP-CIDR6,2001:db8::/32", "2001:db8::1", "classical").unwrap());
        assert!(!rule_set_contains("10.0.0.0/8", "11.2.3.4", "ipcidr").unwrap());
    }

    #[test]
    fn detects_rule_set_format_from_payload() {
        assert_eq!(
            detect_rule_set_format(
                r#"
payload:
  - '+.google.com'
"#
            ),
            "yaml"
        );
        assert_eq!(
            detect_rule_set_format(".google.com\nDOMAIN-KEYWORD,netflix"),
            "text"
        );
    }

    #[test]
    fn match_rule_value_is_normalized_to_any() {
        let input = RuleInput {
            rule_type: "MATCH".into(),
            value: String::new(),
            policy: "DIRECT".into(),
            ..RuleInput::default()
        };

        assert_eq!(normalized_rule_value(&input), "ANY");
        assert!(validate_rule_input(&input).is_ok());
    }

    #[test]
    fn invalid_rule_type_is_rejected() {
        let input = RuleInput {
            rule_type: "SCRIPT".into(),
            value: "x".into(),
            policy: "DIRECT".into(),
            ..RuleInput::default()
        };

        assert!(validate_rule_input(&input).is_err());
    }

    #[test]
    fn invalid_rule_policy_fails_closed() {
        let available = ["Available".to_string()].into_iter().collect();

        assert_eq!(sanitize_rule_policy("Missing", &available), BUILTIN_REJECT);
        assert_eq!(sanitize_rule_policy("Available", &available), "Available");
        assert_eq!(
            sanitize_rule_policy(BUILTIN_DIRECT, &available),
            BUILTIN_DIRECT
        );
    }

    #[tokio::test]
    async fn rule_set_creation_rollback_removes_the_record() {
        let temp = TestDir::new("rule-set-rollback");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app directories");
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        let service = RuleService::new(storage.clone(), paths);
        storage
            .create_rule_set(
                "rs_rollback_test",
                "rollback-test",
                "https://example.com/rules.txt",
                3600,
                Some("domain"),
                "text",
            )
            .await
            .expect("create rule set record");
        service
            .rollback_rule_set_creation("rs_rollback_test")
            .await
            .expect("roll back rule set record");

        assert!(storage
            .list_rule_sets()
            .await
            .expect("list rule sets")
            .into_iter()
            .all(|rule_set| rule_set.name != "rollback-test"));
    }

    #[tokio::test]
    async fn rule_test_sanitizes_policies_and_missing_snapshots_fail_closed() {
        let temp = TestDir::new("rule-test-fail-closed");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app directories");
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        let service = RuleService::new(storage.clone(), paths);
        let invalid_policy_rule = storage
            .upsert_rule(
                Some("rule_invalid_policy".into()),
                "DOMAIN",
                "example.com",
                "Missing Group",
                None,
                true,
            )
            .await
            .expect("create invalid policy rule");

        let result = service
            .test_rule(RuleTestRequest {
                target: "example.com".into(),
            })
            .await
            .expect("test invalid policy rule");
        assert_eq!(result.hit_rule.id, invalid_policy_rule.id);
        assert_eq!(result.final_proxy, BUILTIN_REJECT);

        storage
            .delete_rule(&invalid_policy_rule.id)
            .await
            .expect("delete invalid policy rule");
        let missing_snapshot_rule = storage
            .upsert_rule(
                Some("rule_missing_snapshot".into()),
                "RULE-SET",
                "not-downloaded",
                BUILTIN_DIRECT,
                None,
                true,
            )
            .await
            .expect("create missing snapshot rule");
        let result = service
            .test_rule(RuleTestRequest {
                target: "anything.example".into(),
            })
            .await
            .expect("test missing snapshot rule");
        assert_eq!(result.hit_rule.id, missing_snapshot_rule.id);
        assert_eq!(result.final_proxy, BUILTIN_REJECT);
    }

    #[tokio::test]
    async fn rule_test_and_runtime_share_raw_json_policy_availability() {
        let temp = TestDir::new("rule-test-runtime-availability");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app directories");
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        storage
            .upsert_proxy_item(&test_runtime_item("Broken JSON", "node", Some("{")))
            .await
            .expect("store malformed node");
        storage
            .upsert_proxy_item(&test_runtime_item("Missing JSON", "node", None))
            .await
            .expect("store node without runtime JSON");
        storage
            .upsert_proxy_item(&test_runtime_item("Unavailable Group", "group", None))
            .await
            .expect("store dependent group");
        storage
            .replace_group_members(
                "Unavailable Group",
                &["Broken JSON".into(), "Missing JSON".into()],
            )
            .await
            .expect("store unavailable members");
        storage
            .upsert_rule(
                Some("rule_unavailable_runtime_group".into()),
                "DOMAIN",
                "unavailable.example",
                "Unavailable Group",
                None,
                true,
            )
            .await
            .expect("store unavailable policy rule");

        let service = RuleService::new(storage.clone(), paths.clone());
        let result = service
            .test_rule(RuleTestRequest {
                target: "unavailable.example".into(),
            })
            .await
            .expect("test unavailable runtime policy");
        assert_eq!(result.final_proxy, BUILTIN_REJECT);

        let runtime_path = crate::runtime::compile_runtime_yaml(
            &storage,
            &paths,
            &crate::types::SystemConfig::default(),
        )
        .await
        .expect("compile runtime");
        let runtime = tokio::fs::read_to_string(runtime_path)
            .await
            .expect("read runtime");
        assert!(runtime.contains("DOMAIN,unavailable.example,REJECT"));
        assert!(!runtime.contains("Unavailable Group"));
    }

    fn test_runtime_item(
        name: &str,
        kind: &str,
        raw_json: Option<&str>,
    ) -> crate::storage::ProxyItemRecord {
        crate::storage::ProxyItemRecord {
            name: name.into(),
            kind: kind.into(),
            subscription_id: None,
            display_name: name.into(),
            source: "test".into(),
            builtin: false,
            source_name: None,
            protocol: (kind == "node").then(|| "ss".into()),
            country: None,
            group_type: (kind == "group").then(|| "select".into()),
            raw_json: raw_json.map(str::to_string),
            content_hash: None,
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
