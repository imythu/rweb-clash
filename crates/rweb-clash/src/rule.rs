use crate::error::AppError;
use crate::paths::AppPaths;
use crate::platform::current_system_proxy_url;
use crate::remote;
use crate::runtime::available_policy_targets;
use crate::storage::{RuleSetRefreshState, Storage};
use crate::types::{
    RuleInput, RuleResponse, RuleSetInput, RuleSetResponse, RuleTestRequest, RuleTestResponse,
    BUILTIN_DIRECT, BUILTIN_GLOBAL, BUILTIN_PROXY, BUILTIN_REJECT,
};
use crate::util::{contains_rule_delimiter_or_control, content_hash, new_id, validate_url};
use ipnet::IpNet;
use std::collections::HashSet;
use std::ffi::OsStr;
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
    snapshot_mutation: Arc<Mutex<()>>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct OrphanSnapshotCleanupReport {
    pub(crate) removed: usize,
    pub(crate) failed: usize,
}

impl RuleService {
    pub fn new(storage: Storage, paths: AppPaths) -> Self {
        Self {
            storage,
            paths,
            snapshot_mutation: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) async fn cleanup_orphan_snapshots(
        &self,
    ) -> Result<OrphanSnapshotCleanupReport, AppError> {
        let _snapshot_mutation = self.snapshot_mutation.lock().await;
        let active_snapshot_names = self
            .storage
            .rule_set_snapshot_paths()
            .await?
            .into_iter()
            .filter_map(|local_path| self.managed_snapshot_path(&local_path))
            .filter_map(|path| path.file_name().map(OsStr::to_os_string))
            .collect::<HashSet<_>>();
        let mut entries = tokio::fs::read_dir(&self.paths.rule_sets_dir)
            .await
            .map_err(|error| {
                AppError::internal(format!(
                    "failed to scan rule-set snapshot directory {}: {error}",
                    AppPaths::display(&self.paths.rule_sets_dir)
                ))
            })?;
        let mut report = OrphanSnapshotCleanupReport::default();

        loop {
            let entry = entries.next_entry().await.map_err(|error| {
                AppError::internal(format!(
                    "failed to inspect rule-set snapshot directory {}: {error}",
                    AppPaths::display(&self.paths.rule_sets_dir)
                ))
            })?;
            let Some(entry) = entry else {
                break;
            };
            let file_name = entry.file_name();
            let file_name_text = file_name.to_string_lossy();
            let is_current_download_temp = file_name_text.starts_with(".rule-set-download-")
                && file_name_text.ends_with(".tmp");
            let is_legacy_download_temp = file_name_text.ends_with(".tmp")
                && file_name_text
                    .split_once(".download_")
                    .is_some_and(|(id, suffix)| !id.is_empty() && suffix.len() > ".tmp".len());
            let is_download_temp = is_current_download_temp || is_legacy_download_temp;
            let is_snapshot =
                std::path::Path::new(&file_name).extension() == Some(OsStr::new("list"));
            if !is_snapshot && !is_download_temp {
                continue;
            }
            match entry.file_type().await {
                Ok(file_type) if file_type.is_file() => {}
                Ok(_) => continue,
                Err(error) => {
                    report.failed += 1;
                    warn!(
                        %error,
                        path = %AppPaths::display(&entry.path()),
                        "failed to inspect a possible orphan rule-set snapshot"
                    );
                    continue;
                }
            }
            if is_download_temp {
                match tokio::fs::remove_file(entry.path()).await {
                    Ok(()) => report.removed += 1,
                    Err(error) => {
                        report.failed += 1;
                        warn!(
                            %error,
                            path = %AppPaths::display(&entry.path()),
                            "failed to remove a stale rule-set download"
                        );
                    }
                }
                continue;
            }
            if active_snapshot_names.contains(&file_name) {
                continue;
            }
            match tokio::fs::remove_file(entry.path()).await {
                Ok(()) => report.removed += 1,
                Err(error) => {
                    report.failed += 1;
                    warn!(
                        %error,
                        path = %AppPaths::display(&entry.path()),
                        "failed to remove an orphan rule-set snapshot"
                    );
                }
            }
        }

        Ok(report)
    }

    fn managed_snapshot_path(&self, local_path: &str) -> Option<std::path::PathBuf> {
        let resolved = self.paths.resolve_local_path(local_path);
        let file_name = resolved.file_name()?;
        (self.paths.rule_sets_dir.join(file_name) == resolved).then_some(resolved)
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
                input.policy.trim(),
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
                input.policy.trim(),
                input.desc.as_deref(),
                input.enabled.unwrap_or(true),
                input.position,
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
        let name = input.name.trim();
        if name.is_empty() {
            return Err(AppError::bad_request(
                "ruleset_invalid",
                "rule set name cannot be empty",
            ));
        }
        if contains_rule_delimiter_or_control(name) {
            return Err(AppError::bad_request(
                "ruleset_invalid",
                "rule set name cannot contain commas or control characters",
            ));
        }
        let url = input.url.trim();
        if !validate_url(url) {
            return Err(AppError::bad_request(
                "ruleset_invalid_url",
                "rule set url must start with http:// or https://",
            ));
        }
        let behavior = input
            .behavior
            .as_deref()
            .unwrap_or("classical")
            .trim()
            .to_ascii_lowercase();
        if !matches!(behavior.as_str(), "domain" | "ipcidr" | "classical") {
            return Err(AppError::bad_request(
                "ruleset_invalid",
                format!("unsupported rule set behavior {behavior}"),
            ));
        }
        let format = input
            .format
            .as_deref()
            .unwrap_or("text")
            .trim()
            .to_ascii_lowercase();
        if !matches!(format.as_str(), "text" | "yaml") {
            return Err(AppError::bad_request(
                "ruleset_invalid",
                format!("unsupported rule set format {format}"),
            ));
        }
        let id = new_id("rs");
        info!(
            rule_set_id = %id,
            name,
            %behavior,
            %format,
            interval_seconds = input.interval_seconds(),
            url_host = %url_host_label(url),
            "creating rule set"
        );
        self.storage
            .create_pending_rule_set_with_route(
                &id,
                name,
                url,
                input.interval_seconds(),
                Some(&behavior),
                &format,
                input.download_route,
            )
            .await?;
        let creation = async {
            self.refresh_rule_set(&id).await?;
            self.storage
                .rule_set_including_staged(&id)
                .await?
                .ok_or_else(|| AppError::internal("created rule set disappeared"))
        }
        .await;
        match creation {
            Ok(rule_set) => Ok(rule_set),
            Err(create_error) => {
                warn!(
                    rule_set_id = %id,
                    error = %create_error,
                    "initial rule set staging failed, rolling back record"
                );
                if let Err(rollback_error) = self.rollback_rule_set_creation(&id).await {
                    return Err(AppError::internal(format!(
                        "initial rule set staging failed ({create_error}); rollback failed: {rollback_error}"
                    )));
                }
                Err(create_error)
            }
        }
    }

    async fn rollback_rule_set_creation(&self, id: &str) -> Result<(), AppError> {
        let _snapshot_mutation = self.snapshot_mutation.lock().await;
        let local_paths = self.storage.rule_set_snapshot_paths_for_id(id).await?;
        self.storage.delete_rule_set(id).await?;
        self.remove_rule_set_snapshots(id, &local_paths).await;
        Ok(())
    }

    pub async fn delete_rule_set(&self, id: &str) -> Result<(), AppError> {
        let _snapshot_mutation = self.snapshot_mutation.lock().await;
        let local_paths = self.storage.rule_set_snapshot_paths_for_id(id).await?;
        self.storage.delete_rule_set(id).await?;
        self.remove_rule_set_snapshots(id, &local_paths).await;
        Ok(())
    }

    async fn remove_rule_set_snapshots(&self, id: &str, local_paths: &[String]) {
        let mut snapshots = vec![self.paths.rule_sets_dir.join(format!("{id}.list"))];
        for local_path in local_paths {
            let Some(snapshot) = self.managed_snapshot_path(local_path) else {
                continue;
            };
            if !snapshots.contains(&snapshot) {
                snapshots.push(snapshot);
            }
        }
        for snapshot in snapshots {
            if let Err(error) = tokio::fs::remove_file(&snapshot).await {
                if error.kind() != std::io::ErrorKind::NotFound {
                    warn!(
                        rule_set_id = %id,
                        path = %AppPaths::display(&snapshot),
                        %error,
                        "rule set record was deleted but its snapshot could not be removed"
                    );
                }
            }
        }
    }

    pub async fn refresh_rule_set(&self, id: &str) -> Result<RuleSetRefreshState, AppError> {
        if self.storage.rule_set_for_refresh(id).await?.is_none() {
            return Err(AppError::not_found(
                "ruleset_not_found",
                format!("rule set {id} not found"),
            ));
        }
        let _snapshot_mutation = self.snapshot_mutation.lock().await;
        let result = self.refresh_rule_set_locked(id).await;
        if let Err(error) = &result {
            if let Err(mark_error) = self
                .storage
                .mark_rule_set_refresh_error(id, &error.message)
                .await
            {
                warn!(
                    rule_set_id = %id,
                    %mark_error,
                    "failed to persist a rule-set refresh error"
                );
            }
        }
        result
    }

    async fn refresh_rule_set_locked(&self, id: &str) -> Result<RuleSetRefreshState, AppError> {
        let rule_set = self
            .storage
            .rule_set_for_refresh(id)
            .await?
            .ok_or_else(|| {
                AppError::not_found("ruleset_not_found", format!("rule set {id} not found"))
            })?;
        info!(
            rule_set_id = %id,
            name = %rule_set.name,
            url_host = %url_host_label(&rule_set.url),
            "fetching rule set"
        );
        let config = self.storage.load_config().await?;
        let system_proxy = match current_system_proxy_url().await {
            Ok(proxy) => proxy,
            Err(error) => {
                warn!(%error, "failed to inspect the system proxy download route");
                None
            }
        };
        let response = remote::get_text_routed(
            &rule_set.url,
            None,
            MAX_RULE_SET_BYTES,
            "ruleset_fetch_failed",
            rule_set.download_route,
            remote::RouteOptions {
                core_proxy: Some(format!("http://127.0.0.1:{}", config.mixed_port)),
                system_proxy,
            },
        )
        .await?;
        let used_route = response.route.clone();
        let status = response.status;
        let content = response.body;
        let behavior = rule_set
            .behavior
            .as_deref()
            .unwrap_or("classical")
            .to_string();
        let (content, rule_count, detected_format) = tokio::task::spawn_blocking(move || {
            let (rule_count, detected_format) = validate_rule_set_payload(&content, &behavior)?;
            Ok::<_, AppError>((content, rule_count, detected_format))
        })
        .await
        .map_err(|error| AppError::internal(format!("rule-set analyzer task failed: {error}")))??;
        info!(
            rule_set_id = %id,
            status = status.as_u16(),
            bytes = content.len(),
            rules = rule_count,
            format = %detected_format,
            route = %used_route,
            "rule set fetched"
        );
        let digest = content_hash(content.as_bytes());
        let relative = self.paths.rule_set_version_relative_path(id, &digest);
        let local = self.paths.resolve_local_path(&relative);
        let tmp = self
            .paths
            .rule_sets_dir
            .join(format!(".rule-set-download-{}.tmp", new_id("download")));
        let temporary = RuleSetDownloadTemp(tmp);
        tokio::fs::write(temporary.path(), &content).await?;
        let (destination_existed, existing_matches) = match tokio::fs::metadata(&local).await {
            Ok(metadata) if metadata.is_file() => {
                let matches = metadata.len() == content.len() as u64
                    && content_hash(&tokio::fs::read(&local).await?) == digest;
                (true, matches)
            }
            Ok(_) => {
                return Err(AppError::internal(format!(
                    "rule-set snapshot destination is not a regular file: {}",
                    AppPaths::display(&local)
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (false, false),
            Err(error) => return Err(AppError::from(error)),
        };
        let installed_by_this_attempt = !destination_existed;
        if !existing_matches {
            replace_downloaded_file(temporary.path(), &local).await?;
        }
        let previous = match self
            .storage
            .stage_rule_set_refresh(
                id,
                &relative,
                content.len() as u64,
                rule_count,
                &digest,
                detected_format,
                None,
            )
            .await
        {
            Ok(previous) => previous,
            Err(error) => {
                if installed_by_this_attempt {
                    let _ = tokio::fs::remove_file(&local).await;
                }
                return Err(error);
            }
        };
        info!(
            rule_set_id = %id,
            local_path = %relative,
            format = %detected_format,
            "rule set saved"
        );
        self.storage
            .set_rule_set_last_route(id, &used_route)
            .await?;
        Ok(previous)
    }

    pub async fn restore_rule_set_refresh(
        &self,
        id: &str,
        previous: &RuleSetRefreshState,
    ) -> Result<(), AppError> {
        let _snapshot_mutation = self.snapshot_mutation.lock().await;
        self.storage.restore_rule_set_refresh(id, previous).await
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

struct RuleSetDownloadTemp(std::path::PathBuf);

impl RuleSetDownloadTemp {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for RuleSetDownloadTemp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
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
    if input.rule_type != "MATCH" && contains_rule_delimiter_or_control(input.value.trim()) {
        return Err(AppError::bad_request(
            "rule_invalid",
            "rule value cannot contain commas or control characters",
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
    if contains_rule_delimiter_or_control(input.policy.trim()) {
        return Err(AppError::bad_request(
            "rule_invalid",
            "rule policy cannot contain commas or control characters",
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

#[cfg(test)]
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

fn validate_rule_set_payload(
    content: &str,
    behavior: &str,
) -> Result<(u64, &'static str), AppError> {
    if content
        .chars()
        .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(AppError::bad_request(
            "ruleset_invalid_payload",
            "rule set contains unsupported control characters",
        ));
    }
    let trimmed = content.trim_start();
    let lowercase_prefix = trimmed
        .chars()
        .take(256)
        .collect::<String>()
        .to_ascii_lowercase();
    if lowercase_prefix.starts_with("<!doctype html")
        || lowercase_prefix.starts_with("<html")
        || lowercase_prefix.contains("<body")
    {
        return Err(AppError::bad_request(
            "ruleset_invalid_payload",
            "rule set response is an HTML document",
        ));
    }

    let detected_format = detect_rule_set_format(content);
    let entries = if detected_format == "yaml" {
        let value = serde_yaml::from_str::<serde_yaml::Value>(content)?;
        let payload = value
            .as_mapping()
            .and_then(|mapping| mapping.get(serde_yaml::Value::String("payload".into())))
            .and_then(serde_yaml::Value::as_sequence)
            .ok_or_else(|| {
                AppError::bad_request(
                    "ruleset_invalid_payload",
                    "YAML rule set must contain a payload sequence",
                )
            })?;
        payload
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .map(|entry| entry.trim().to_string())
                    .ok_or_else(|| {
                        AppError::bad_request(
                            "ruleset_invalid_payload",
                            "YAML rule-set payload entries must be strings",
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        content
            .lines()
            .filter_map(normalize_rule_set_line)
            .map(str::to_string)
            .collect()
    };
    if entries.is_empty() || entries.iter().any(|entry| entry.is_empty()) {
        return Err(AppError::bad_request(
            "ruleset_invalid_payload",
            "rule set contains no rules",
        ));
    }
    for line in &entries {
        let valid = match behavior {
            "domain" => valid_domain_rule_set_entry(line),
            "ipcidr" => line.parse::<IpNet>().is_ok(),
            "classical" => valid_classical_rule_set_entry(line),
            _ => false,
        };
        if !valid {
            return Err(AppError::bad_request(
                "ruleset_invalid_payload",
                format!("invalid {behavior} rule-set entry: {line}"),
            ));
        }
    }
    Ok((entries.len() as u64, detected_format))
}

fn valid_domain_rule_set_entry(value: &str) -> bool {
    !value.contains(char::is_whitespace)
        && !value.contains("//")
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ".-_+*".contains(ch))
}

fn valid_classical_rule_set_entry(value: &str) -> bool {
    let Some((kind, remainder)) = value.split_once(',') else {
        return false;
    };
    let rule_value = remainder.split(',').next().unwrap_or_default().trim();
    if rule_value.is_empty() {
        return false;
    }
    match kind.trim() {
        "DOMAIN" | "DOMAIN-SUFFIX" => valid_domain_rule_set_entry(rule_value),
        "DOMAIN-KEYWORD" => !rule_value.contains(char::is_whitespace),
        "IP-CIDR" | "IP-CIDR6" => rule_value.parse::<IpNet>().is_ok(),
        "GEOIP" => rule_value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-'),
        _ => false,
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
    fn rule_set_payload_validation_rejects_error_documents_and_malformed_entries() {
        assert_eq!(
            validate_rule_set_payload("payload:\n  - '+.example.com'\n", "domain")
                .expect("valid domain payload"),
            (1, "yaml")
        );
        assert_eq!(
            validate_rule_set_payload("payload: ['+.example.com']\n", "domain")
                .expect("valid inline YAML payload"),
            (1, "yaml")
        );
        assert!(validate_rule_set_payload("10.0.0.0/8\n2001:db8::/32\n", "ipcidr").is_ok());
        assert!(validate_rule_set_payload(
            "DOMAIN-SUFFIX,example.com\nIP-CIDR,10.0.0.0/8\n",
            "classical"
        )
        .is_ok());
        for (content, behavior) in [
            (
                "<!doctype html><html><body>gateway error</body></html>",
                "domain",
            ),
            ("# comments only\n", "domain"),
            ("payload:\n  - key: value\n", "domain"),
            ("10.0.0.0/99\n", "ipcidr"),
            ("upstream temporarily unavailable\n", "classical"),
            ("NOT-A-MIHOMO-RULE,anything\n", "classical"),
        ] {
            assert_eq!(
                validate_rule_set_payload(content, behavior)
                    .expect_err("reject malformed rule-set response")
                    .code,
                "ruleset_invalid_payload"
            );
        }
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
    fn rule_fields_reject_clash_delimiters_and_control_characters() {
        for input in [
            RuleInput {
                value: "example.com,DIRECT".into(),
                ..RuleInput::default()
            },
            RuleInput {
                policy: "Group,One".into(),
                ..RuleInput::default()
            },
            RuleInput {
                value: "example.com\nMATCH".into(),
                ..RuleInput::default()
            },
        ] {
            assert_eq!(
                validate_rule_input(&input)
                    .expect_err("reject ambiguous rule serialization")
                    .code,
                "rule_invalid"
            );
        }
    }

    #[tokio::test]
    async fn rule_set_inputs_reject_invalid_names_and_enums_before_fetching() {
        let temp = TestDir::new("rule-set-input-validation");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app directories");
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        let service = RuleService::new(storage, paths);
        for input in [
            RuleSetInput {
                name: "bad,name".into(),
                url: "https://example.com/rules.txt".into(),
                ..RuleSetInput::default()
            },
            RuleSetInput {
                name: "bad-behavior".into(),
                url: "https://example.com/rules.txt".into(),
                behavior: Some("domains".into()),
                ..RuleSetInput::default()
            },
            RuleSetInput {
                name: "bad-format".into(),
                url: "https://example.com/rules.mrs".into(),
                format: Some("mrs".into()),
                ..RuleSetInput::default()
            },
        ] {
            assert_eq!(
                service
                    .create_rule_set(input)
                    .await
                    .expect_err("reject invalid rule set input before network access")
                    .code,
                "ruleset_invalid"
            );
        }
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
    async fn rule_set_deletion_removes_its_downloaded_snapshot() {
        let temp = TestDir::new("rule-set-snapshot-delete");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app directories");
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        let service = RuleService::new(storage.clone(), paths.clone());
        let id = "rs_snapshot_delete";
        storage
            .create_rule_set(
                id,
                "snapshot-delete",
                "https://example.com/rules.txt",
                3_600,
                Some("domain"),
                "text",
            )
            .await
            .expect("create rule set record");
        let snapshot = paths.rule_sets_dir.join(format!("{id}.version.list"));
        tokio::fs::write(&snapshot, "example.com\n")
            .await
            .expect("write rule set snapshot");
        storage
            .update_rule_set_refresh(
                id,
                &paths.rule_set_version_relative_path(id, "version"),
                12,
                1,
                "version",
                "text",
                None,
            )
            .await
            .expect("record versioned snapshot");

        let snapshot_guard = service.snapshot_mutation.lock().await;
        let delete_service = service.clone();
        let mut deletion = tokio::spawn(async move { delete_service.delete_rule_set(id).await });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), &mut deletion)
                .await
                .is_err(),
            "deletion must wait for an in-flight refresh"
        );
        drop(snapshot_guard);
        deletion
            .await
            .expect("join deletion task")
            .expect("delete rule set and snapshot");

        assert!(!snapshot.exists());
        assert!(storage
            .list_rule_sets()
            .await
            .expect("list rule sets")
            .into_iter()
            .all(|rule_set| rule_set.id != id));
    }

    #[tokio::test]
    async fn orphan_snapshot_cleanup_only_removes_unreferenced_regular_list_files() {
        let temp = TestDir::new("rule-set-orphan-cleanup");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app directories");
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        let active_id = "rs_active_snapshot";
        storage
            .create_rule_set(
                active_id,
                "active-snapshot",
                "https://example.com/active.txt",
                3_600,
                Some("domain"),
                "text",
            )
            .await
            .expect("create active rule set record");

        let active = paths.rule_sets_dir.join(format!("{active_id}.v1.list"));
        let orphan = paths.rule_sets_dir.join("rs_active_snapshot_old.list");
        let temporary = paths.rule_sets_dir.join("rs_interrupted.list.tmp");
        let stale_download = paths
            .rule_sets_dir
            .join(".rule-set-download-download_stale.tmp");
        let legacy_stale_download = paths
            .rule_sets_dir
            .join("rs_legacy.download_download_stale.tmp");
        let wrong_extension = paths.rule_sets_dir.join("rs_orphan.LIST");
        let matching_directory = paths.rule_sets_dir.join("rs_directory.list");
        for file in [
            &active,
            &orphan,
            &temporary,
            &stale_download,
            &legacy_stale_download,
            &wrong_extension,
        ] {
            tokio::fs::write(file, "example.com\n")
                .await
                .expect("write snapshot cleanup fixture");
        }
        storage
            .update_rule_set_refresh(
                active_id,
                &paths.rule_set_version_relative_path(active_id, "v1"),
                12,
                1,
                "active-hash",
                "text",
                None,
            )
            .await
            .expect("mark the active snapshot in storage");
        tokio::fs::create_dir(&matching_directory)
            .await
            .expect("create matching directory");
        let nested_snapshot = matching_directory.join("nested.list");
        tokio::fs::write(&nested_snapshot, "keep nested\n")
            .await
            .expect("write nested snapshot");

        #[cfg(unix)]
        let (matching_symlink, symlink_target) = {
            use std::os::unix::fs::symlink;

            let target = paths.rule_sets_dir.join("symlink-target.txt");
            let link = paths.rule_sets_dir.join("rs_symlink.list");
            std::fs::write(&target, b"keep target\n").expect("write symlink target");
            symlink(&target, &link).expect("create snapshot symlink");
            (link, target)
        };

        let service = RuleService::new(storage, paths);
        let report = service
            .cleanup_orphan_snapshots()
            .await
            .expect("clean orphan snapshots");

        assert_eq!(
            report,
            OrphanSnapshotCleanupReport {
                removed: 3,
                failed: 0,
            }
        );
        assert!(!orphan.exists());
        assert!(!stale_download.exists());
        assert!(!legacy_stale_download.exists());
        for preserved in [
            &active,
            &temporary,
            &wrong_extension,
            &matching_directory,
            &nested_snapshot,
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
    fn rule_set_download_guard_removes_partial_files() {
        let temp = TestDir::new("rule-set-download-guard");
        let temporary = temp.path().join("partial.tmp");
        std::fs::write(&temporary, b"partial").expect("write partial download");
        {
            let guard = RuleSetDownloadTemp(temporary.clone());
            assert_eq!(guard.path(), temporary);
        }
        assert!(!temporary.exists());
    }

    #[tokio::test]
    async fn orphan_snapshot_cleanup_reports_an_unreadable_snapshot_directory() {
        let temp = TestDir::new("rule-set-orphan-cleanup-error");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app directories");
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        tokio::fs::remove_dir(&paths.rule_sets_dir)
            .await
            .expect("remove rule-set directory");
        tokio::fs::write(&paths.rule_sets_dir, "not a directory")
            .await
            .expect("replace rule-set directory with a file");

        let error = RuleService::new(storage, paths)
            .cleanup_orphan_snapshots()
            .await
            .expect_err("surface snapshot directory scan failure");

        assert!(error
            .message
            .contains("failed to scan rule-set snapshot directory"));
    }

    #[tokio::test]
    async fn rule_test_missing_snapshots_fail_closed() {
        let temp = TestDir::new("rule-test-fail-closed");
        let paths = AppPaths::from_root(temp.path());
        paths.ensure_dirs().expect("create app directories");
        let storage = Storage::connect(&paths)
            .await
            .expect("connect test storage");
        let service = RuleService::new(storage.clone(), paths);
        storage
            .create_rule_set(
                "rs_not_downloaded",
                "not-downloaded",
                "https://example.com/not-downloaded.txt",
                3_600,
                Some("domain"),
                "text",
            )
            .await
            .expect("create rule set without a snapshot");
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
            .upsert_proxy_item(&test_runtime_item("Broken JSON", "node", Some("{}")))
            .await
            .expect("store initially valid node");
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
            .expect("store dependent members");
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
            .expect("store initially available policy rule");
        storage
            .upsert_proxy_item(&test_runtime_item("Broken JSON", "node", Some("{")))
            .await
            .expect("corrupt the last valid member after the rule was stored");

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
