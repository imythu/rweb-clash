use crate::error::AppError;
use crate::storage::{ProxyItemRecord, Storage};
use crate::types::{
    GroupFilterInput, ProxyGroupRequest, ProxyNodeResponse, BUILTIN_DIRECT, BUILTIN_GLOBAL,
    BUILTIN_PROXY, BUILTIN_REJECT,
};
use crate::util::{contains_rule_delimiter_or_control, content_hash};
use regex::Regex;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct ProxyService {
    storage: Storage,
}

impl ProxyService {
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    pub async fn create_group(&self, mut request: ProxyGroupRequest) -> Result<(), AppError> {
        normalize_group_request(&mut request);
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
        mut request: ProxyGroupRequest,
    ) -> Result<(), AppError> {
        normalize_group_request(&mut request);
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
                "proxy groups referenced by routing rules cannot be renamed",
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
        info!(
            group = %request.name,
            group_type = %request.group_type,
            filters = request.filter.len(),
            "saving custom proxy group"
        );
        let item = ProxyItemRecord {
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
            strategy_json: "{}".into(),
            position: 100_000,
            enabled: true,
        };
        self.storage
            .save_custom_group(old_name.as_deref(), &item, &request.filter)
            .await
    }
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
        _ => {
            if matches!(filter.operator.as_str(), "equals" | "in") {
                node.name.clone()
            } else {
                node.display_name.clone()
            }
        }
    };
    let filter_value = if filter.field == "status" && filter.value.trim().is_empty() {
        "timeout"
    } else {
        &filter.value
    };
    match filter.operator.as_str() {
        "equals" => {
            if filter.has_values() {
                filter
                    .effective_values()
                    .into_iter()
                    .any(|value| target.eq_ignore_ascii_case(value))
            } else {
                target.eq_ignore_ascii_case(filter_value)
            }
        }
        "is" => target.eq_ignore_ascii_case(filter_value),
        "in" => filter
            .effective_values()
            .into_iter()
            .any(|value| target.eq_ignore_ascii_case(value)),
        "regex" => Regex::new(filter_value)
            .map(|regex| regex.is_match(&target))
            .unwrap_or(false),
        "less_than" => target
            .parse::<i64>()
            .ok()
            .zip(filter_value.parse::<i64>().ok())
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
            .starts_with(&filter_value.to_ascii_lowercase()),
        _ => target
            .to_ascii_lowercase()
            .contains(&filter_value.to_ascii_lowercase()),
    }
}

fn validate_group_request(request: &ProxyGroupRequest) -> Result<(), AppError> {
    if request.name.trim().is_empty() {
        return Err(AppError::bad_request(
            "proxy_group_invalid",
            "proxy group name cannot be empty",
        ));
    }
    if matches!(
        request.name.to_ascii_uppercase().as_str(),
        BUILTIN_DIRECT | BUILTIN_REJECT | BUILTIN_GLOBAL | BUILTIN_PROXY
    ) {
        return Err(AppError::conflict(
            "proxy_group_reserved",
            format!("{} is a reserved proxy policy name", request.name),
        ));
    }
    if contains_rule_delimiter_or_control(&request.name) {
        return Err(AppError::bad_request(
            "proxy_group_invalid",
            "proxy group name cannot contain commas or control characters",
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
        if !matches!(filter.action.trim(), "keep" | "discard") {
            return Err(AppError::bad_request(
                "proxy_group_invalid_filter",
                format!("unsupported proxy group filter action {}", filter.action),
            ));
        }
        if !matches!(
            filter.field.trim(),
            "name" | "country" | "protocol" | "latency" | "status" | "subscription"
        ) {
            return Err(AppError::bad_request(
                "proxy_group_invalid_filter",
                format!("unsupported proxy group filter field {}", filter.field),
            ));
        }
        if !matches!(
            filter.operator.trim(),
            "contains" | "equals" | "in" | "regex" | "is" | "less_than" | "starts_with"
        ) {
            return Err(AppError::bad_request(
                "proxy_group_invalid_filter",
                format!(
                    "unsupported proxy group filter operator {}",
                    filter.operator
                ),
            ));
        }
        if !filter.enabled.unwrap_or(true) {
            continue;
        }
        let supports_values = matches!(filter.operator.as_str(), "in" | "equals");
        let has_match_value = !filter.value.is_empty() || (supports_values && filter.has_values());
        if !has_match_value && filter.field != "status" {
            return Err(AppError::bad_request(
                "proxy_group_invalid_filter",
                "proxy group filter value cannot be empty",
            ));
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

fn normalize_group_request(request: &mut ProxyGroupRequest) {
    request.name = request.name.trim().to_string();
    request.group_type = request.group_type.trim().to_string();
    for filter in &mut request.filter {
        filter.action = filter.action.trim().to_string();
        filter.field = filter.field.trim().to_string();
        filter.operator = filter.operator.trim().to_string();
        filter.value = filter.value.trim().to_string();
        filter.values = filter
            .values
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
        if !matches!(filter.operator.as_str(), "in" | "equals") {
            filter.values.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, protocol: &str, latency: i64, country: Option<&str>) -> ProxyNodeResponse {
        ProxyNodeResponse {
            name: name.into(),
            display_name: name.split("^_^").next().unwrap_or(name).into(),
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

    #[test]
    fn display_name_filters_and_implicit_timeout_match_frontend_semantics() {
        let nodes = vec![
            node("香港 01^_^sub_1", "trojan", 80, Some("HK")),
            node("超时节点^_^sub_1", "ss", 0, Some("US")),
        ];
        let display_name = GroupFilterInput {
            action: "keep".into(),
            field: "name".into(),
            operator: "regex".into(),
            value: "^香港 01$".into(),
            ..GroupFilterInput::default()
        };
        assert_eq!(
            calculate_members(&nodes, &[display_name]),
            vec!["香港 01^_^sub_1"]
        );

        let timeout = GroupFilterInput {
            action: "keep".into(),
            field: "status".into(),
            operator: "is".into(),
            value: String::new(),
            ..GroupFilterInput::default()
        };
        assert_eq!(
            calculate_members(&nodes, &[timeout]),
            vec!["超时节点^_^sub_1"]
        );

        let mut delimiter_node = node("A^_^B^_^sub_1", "ss", 50, Some("US"));
        delimiter_node.display_name = "A^_^B".into();
        let delimiter_name = GroupFilterInput {
            action: "keep".into(),
            field: "name".into(),
            operator: "regex".into(),
            value: r"^A\^_\^B$".into(),
            ..GroupFilterInput::default()
        };
        assert_eq!(
            calculate_members(&[delimiter_node], &[delimiter_name]),
            vec!["A^_^B^_^sub_1"]
        );
    }

    #[test]
    fn custom_group_names_reject_reserved_and_rule_delimiter_values() {
        let request = |name: &str| ProxyGroupRequest {
            name: name.into(),
            group_type: "select".into(),
            filter: Vec::new(),
        };
        for name in ["DIRECT", "reject", "Global", "PROXY"] {
            assert_eq!(
                validate_group_request(&request(name))
                    .expect_err("reject reserved proxy policy name")
                    .code,
                "proxy_group_reserved"
            );
        }
        for name in ["Group,One", "Group\nOne"] {
            assert_eq!(
                validate_group_request(&request(name))
                    .expect_err("reject names that break rule serialization")
                    .code,
                "proxy_group_invalid"
            );
        }
    }

    #[test]
    fn group_filter_enums_reject_silent_fallbacks() {
        let request_with = |filter: GroupFilterInput| ProxyGroupRequest {
            name: "Strategy".into(),
            group_type: "select".into(),
            filter: vec![filter],
        };

        for filter in [
            GroupFilterInput {
                action: "kepe".into(),
                value: "HK".into(),
                ..GroupFilterInput::default()
            },
            GroupFilterInput {
                field: "county".into(),
                value: "HK".into(),
                ..GroupFilterInput::default()
            },
            GroupFilterInput {
                operator: "equal".into(),
                value: "HK".into(),
                ..GroupFilterInput::default()
            },
        ] {
            assert_eq!(
                validate_group_request(&request_with(filter))
                    .expect_err("reject an unsupported filter enum")
                    .code,
                "proxy_group_invalid_filter"
            );
        }

        for operator in ["contains", "regex", "less_than", "starts_with"] {
            let filter = GroupFilterInput {
                operator: operator.into(),
                value: String::new(),
                values: vec!["HK".into()],
                ..GroupFilterInput::default()
            };
            assert_eq!(
                validate_group_request(&request_with(filter))
                    .expect_err("unrelated values must not hide an empty filter value")
                    .code,
                "proxy_group_invalid_filter"
            );
        }

        for operator in ["in", "equals"] {
            let filter = GroupFilterInput {
                operator: operator.into(),
                value: String::new(),
                values: vec!["HK".into()],
                ..GroupFilterInput::default()
            };
            assert!(validate_group_request(&request_with(filter)).is_ok());
        }

        let scalar_is = GroupFilterInput {
            operator: "is".into(),
            value: "HK".into(),
            ..GroupFilterInput::default()
        };
        assert!(validate_group_request(&request_with(scalar_is)).is_ok());

        let values_only_is = GroupFilterInput {
            operator: "is".into(),
            value: String::new(),
            values: vec!["HK".into()],
            ..GroupFilterInput::default()
        };
        assert_eq!(
            validate_group_request(&request_with(values_only_is))
                .expect_err("is requires one scalar value")
                .code,
            "proxy_group_invalid_filter"
        );

        let disabled_incomplete_filter = GroupFilterInput {
            field: "latency".into(),
            operator: "less_than".into(),
            value: String::new(),
            enabled: Some(false),
            ..GroupFilterInput::default()
        };
        assert!(validate_group_request(&request_with(disabled_incomplete_filter)).is_ok());
    }
}
