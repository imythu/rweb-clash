use crate::error::AppError;
use crate::storage::{ProxyItemRecord, Storage};
use crate::types::{GroupFilterInput, ProxyGroupRequest, ProxyNodeResponse, BUILTIN_PROXY};
use crate::util::content_hash;
use regex::Regex;
use serde_json::json;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct ProxyService {
    storage: Storage,
}

impl ProxyService {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub async fn create_group(&self, request: ProxyGroupRequest) -> Result<(), AppError> {
        validate_group_request(&request)?;
        if self.storage.group_source(&request.name).await?.is_some() {
            warn!(group = %request.name, "proxy group create rejected because it already exists");
            return Err(AppError::conflict(
                "proxy_group_exists",
                format!("proxy group {} already exists", request.name),
            ));
        }
        self.save_custom_group(None, request).await
    }

    pub async fn update_group(
        &self,
        current_name: &str,
        request: ProxyGroupRequest,
    ) -> Result<(), AppError> {
        validate_group_request(&request)?;
        match self.storage.group_source(current_name).await? {
            Some(source) if source == "custom" => {}
            Some(_) => {
                warn!(group = %current_name, "proxy group update rejected because group is read-only");
                return Err(AppError::conflict(
                    "proxy_group_readonly",
                    "subscription managed proxy groups are read-only",
                ));
            }
            None => {
                warn!(group = %current_name, "proxy group update rejected because group was not found");
                return Err(AppError::not_found(
                    "proxy_group_not_found",
                    format!("proxy group {current_name} not found"),
                ));
            }
        }
        if current_name != request.name && self.storage.group_source(&request.name).await?.is_some()
        {
            warn!(group = %request.name, "proxy group update rejected because target name already exists");
            return Err(AppError::conflict(
                "proxy_group_exists",
                format!("proxy group {} already exists", request.name),
            ));
        }
        if current_name != request.name
            && self.storage.policy_reference_count(current_name).await? > 0
        {
            return Err(AppError::conflict(
                "proxy_group_referenced",
                "proxy groups referenced by enabled routing rules cannot be renamed",
            ));
        }
        self.save_custom_group(Some(current_name.to_string()), request)
            .await
    }

    async fn save_custom_group(
        &self,
        old_name: Option<String>,
        request: ProxyGroupRequest,
    ) -> Result<(), AppError> {
        let nodes = self.storage.all_node_records().await?;
        let members = calculate_members(&nodes, &request.filter);
        if members.is_empty() {
            warn!(
                group = %request.name,
                filters = request.filter.len(),
                nodes = nodes.len(),
                "proxy group filters matched no nodes"
            );
            return Err(AppError::bad_request(
                "proxy_group_empty",
                "custom proxy group filters did not match any nodes",
            ));
        }
        info!(
            group = %request.name,
            group_type = %request.group_type,
            filters = request.filter.len(),
            members = members.len(),
            "saving custom proxy group"
        );
        let strategy = json!({ "now": members.first() }).to_string();
        self.storage
            .upsert_proxy_item(&ProxyItemRecord {
                name: request.name.clone(),
                kind: "group".into(),
                subscription_id: None,
                display_name: request.name.clone(),
                source: "custom".into(),
                builtin: false,
                source_name: None,
                protocol: None,
                country: None,
                group_type: Some(request.group_type.clone()),
                raw_json: None,
                content_hash: Some(content_hash(format!(
                    "{}:{:?}",
                    request.name, request.filter
                ))),
                latency_ms: None,
                alive: true,
                filtered_out: false,
                filter_reason: None,
                delay_ms: None,
                tolerance_ms: Some(50),
                url: Some(crate::types::DEFAULT_DELAY_TEST_URL.into()),
                interval_seconds: Some(300),
                strategy_json: strategy,
                position: 100_000,
                enabled: true,
            })
            .await?;
        self.storage
            .replace_group_filters(&request.name, &request.filter)
            .await?;
        self.storage
            .replace_group_members(&request.name, &members)
            .await?;
        if let Some(old_name) = old_name.as_deref() {
            if old_name != request.name {
                self.storage.delete_custom_group(old_name).await?;
            }
        }
        Ok(())
    }
}

pub async fn reconcile_custom_groups(storage: &Storage) -> Result<(), AppError> {
    let group_names = storage.custom_group_names().await?;
    if group_names.is_empty() {
        return Ok(());
    }
    let nodes = storage.all_node_records().await?;
    for group_name in group_names {
        let filters = storage.group_filters(&group_name).await?;
        let members = calculate_members(&nodes, &filters);
        if members.is_empty() {
            warn!(group = %group_name, "custom proxy group has no members after reconciliation");
        }
        storage.replace_group_members(&group_name, &members).await?;
    }
    Ok(())
}

pub fn calculate_members(nodes: &[ProxyNodeResponse], filters: &[GroupFilterInput]) -> Vec<String> {
    let has_keep = filters
        .iter()
        .any(|filter| filter.enabled.unwrap_or(true) && filter.action == "keep");
    nodes
        .iter()
        .filter(|node| node_matches_filters(node, filters, has_keep))
        .map(|node| node.name.clone())
        .collect()
}

fn node_matches_filters(
    node: &ProxyNodeResponse,
    filters: &[GroupFilterInput],
    has_keep: bool,
) -> bool {
    let mut included = !has_keep;
    for filter in filters
        .iter()
        .filter(|filter| filter.enabled.unwrap_or(true))
    {
        let matched = matches_group_filter(node, filter);
        if !matched {
            continue;
        }
        included = filter.action == "keep";
    }
    included
}

fn matches_group_filter(node: &ProxyNodeResponse, filter: &GroupFilterInput) -> bool {
    let target = match filter.field.as_str() {
        "country" => node.country.clone().unwrap_or_default(),
        "protocol" => node.protocol.clone(),
        "subscription" => node
            .subscription_name
            .clone()
            .unwrap_or_else(|| "LOCAL".into()),
        "latency" => node.latency.to_string(),
        "status" => {
            if node.latency > 0 {
                "online".into()
            } else {
                "timeout".into()
            }
        }
        _ => node.name.clone(),
    };
    match filter.operator.as_str() {
        "equals" | "is" => {
            if filter.has_values() {
                filter
                    .effective_values()
                    .into_iter()
                    .any(|value| target.eq_ignore_ascii_case(value))
            } else {
                target.eq_ignore_ascii_case(&filter.value)
            }
        }
        "in" => filter
            .effective_values()
            .into_iter()
            .any(|value| target.eq_ignore_ascii_case(value)),
        "regex" => Regex::new(&filter.value)
            .map(|regex| regex.is_match(&target))
            .unwrap_or(false),
        "less_than" => target
            .parse::<i64>()
            .ok()
            .zip(filter.value.parse::<i64>().ok())
            .map(|(left, right)| {
                if right == -1 {
                    left <= 0
                } else {
                    left > 0 && left < right
                }
            })
            .unwrap_or(false),
        "starts_with" => target
            .to_ascii_lowercase()
            .starts_with(&filter.value.to_ascii_lowercase()),
        _ => target
            .to_ascii_lowercase()
            .contains(&filter.value.to_ascii_lowercase()),
    }
}

fn validate_group_request(request: &ProxyGroupRequest) -> Result<(), AppError> {
    if request.name.trim().is_empty() {
        return Err(AppError::bad_request(
            "proxy_group_invalid",
            "proxy group name cannot be empty",
        ));
    }
    if request.name.trim() == BUILTIN_PROXY {
        return Err(AppError::conflict(
            "proxy_group_reserved",
            "PROXY is a system builtin proxy group",
        ));
    }
    if !matches!(
        request.group_type.as_str(),
        "select" | "url-test" | "fallback" | "load-balance"
    ) {
        return Err(AppError::bad_request(
            "proxy_group_invalid",
            format!("unsupported proxy group type {}", request.group_type),
        ));
    }
    for filter in &request.filter {
        if filter.id.is_none() && filter.is_value_empty() && filter.field != "status" {
            continue;
        }
        if filter.operator == "regex" && Regex::new(&filter.value).is_err() {
            return Err(AppError::bad_request(
                "proxy_group_invalid_regex",
                format!("invalid regex filter {}", filter.value),
            ));
        }
        if filter.field == "latency" {
            let Ok(value) = filter.value.trim().parse::<i64>() else {
                return Err(AppError::bad_request(
                    "proxy_group_invalid_latency",
                    "latency filter value must be an integer greater than or equal to -1",
                ));
            };
            if value < -1 {
                return Err(AppError::bad_request(
                    "proxy_group_invalid_latency",
                    "latency filter value must be an integer greater than or equal to -1",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, protocol: &str, latency: i64, country: Option<&str>) -> ProxyNodeResponse {
        ProxyNodeResponse {
            name: name.into(),
            protocol: protocol.into(),
            latency,
            country: country.map(str::to_string),
            subscription_id: Some("sub_1".into()),
            subscription_name: Some("Airport".into()),
        }
    }

    #[test]
    fn calculate_members_starts_exclusive_when_keep_filters_exist() {
        let nodes = vec![
            node("香港 01", "trojan", 80, Some("HK")),
            node("日本 01", "ss", 120, Some("JP")),
        ];
        let filters = vec![GroupFilterInput {
            action: "keep".into(),
            field: "country".into(),
            operator: "is".into(),
            value: "HK".into(),
            ..GroupFilterInput::default()
        }];

        assert_eq!(calculate_members(&nodes, &filters), vec!["香港 01"]);
    }

    #[test]
    fn calculate_members_can_discard_after_keep() {
        let nodes = vec![
            node("香港 01", "trojan", 80, Some("HK")),
            node("香港 超时", "trojan", 0, Some("HK")),
        ];
        let filters = vec![
            GroupFilterInput {
                action: "keep".into(),
                field: "country".into(),
                operator: "is".into(),
                value: "HK".into(),
                ..GroupFilterInput::default()
            },
            GroupFilterInput {
                action: "discard".into(),
                field: "status".into(),
                operator: "is".into(),
                value: "timeout".into(),
                ..GroupFilterInput::default()
            },
        ];

        assert_eq!(calculate_members(&nodes, &filters), vec!["香港 01"]);
    }

    #[test]
    fn country_filter_supports_multiple_values() {
        let nodes = vec![
            node("香港 01", "trojan", 80, Some("HK")),
            node("日本 01", "ss", 120, Some("JP")),
            node("美国 01", "ss", 180, Some("US")),
        ];
        let filters = vec![GroupFilterInput {
            action: "keep".into(),
            field: "country".into(),
            operator: "in".into(),
            values: vec!["HK".into(), "JP".into()],
            ..GroupFilterInput::default()
        }];

        assert_eq!(
            calculate_members(&nodes, &filters),
            vec!["香港 01", "日本 01"]
        );
    }

    #[test]
    fn name_filter_supports_multiple_exact_values() {
        let nodes = vec![
            node("香港 01", "trojan", 80, Some("HK")),
            node("日本 01", "ss", 120, Some("JP")),
            node("美国 01", "ss", 180, Some("US")),
        ];
        let filters = vec![GroupFilterInput {
            action: "keep".into(),
            field: "name".into(),
            operator: "in".into(),
            values: vec!["香港 01".into(), "日本 01".into()],
            ..GroupFilterInput::default()
        }];

        assert_eq!(
            calculate_members(&nodes, &filters),
            vec!["香港 01", "日本 01"]
        );
    }

    #[test]
    fn in_filter_keeps_legacy_comma_value_compatibility() {
        let nodes = vec![
            node("香港 01", "trojan", 80, Some("HK")),
            node("日本 01", "ss", 120, Some("JP")),
            node("美国 01", "ss", 180, Some("US")),
        ];
        let filters = vec![GroupFilterInput {
            action: "keep".into(),
            field: "country".into(),
            operator: "in".into(),
            value: "HK,JP".into(),
            ..GroupFilterInput::default()
        }];

        assert_eq!(
            calculate_members(&nodes, &filters),
            vec!["香港 01", "日本 01"]
        );
    }

    #[test]
    fn latency_minus_one_matches_unavailable_nodes() {
        let nodes = vec![
            node("香港 01", "trojan", 80, Some("HK")),
            node("未测速", "ss", -1, Some("JP")),
            node("超时", "ss", 0, Some("US")),
        ];
        let filters = vec![GroupFilterInput {
            action: "keep".into(),
            field: "latency".into(),
            operator: "less_than".into(),
            value: "-1".into(),
            ..GroupFilterInput::default()
        }];

        assert_eq!(calculate_members(&nodes, &filters), vec!["未测速", "超时"]);
    }
}
