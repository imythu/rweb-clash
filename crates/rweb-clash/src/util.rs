use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

pub fn now_iso() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

pub fn display_log_time(value: &str) -> String {
    let mut normalized = value.replace('T', " ");
    if normalized.len() >= 19 {
        normalized.truncate(19);
    }
    normalized
}

pub fn new_id(prefix: &str) -> String {
    format!("{}_{}", prefix, Uuid::new_v4().simple())
}

pub fn content_hash(content: impl AsRef<[u8]>) -> String {
    let digest = Sha256::digest(content.as_ref());
    hex::encode(digest)
}

pub fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

pub fn i64_to_bool(value: i64) -> bool {
    value != 0
}

pub fn normalize_status(status: &str) -> String {
    match status {
        "updating" | "syncing" => "syncing".into(),
        "offline" | "error" => "offline".into(),
        _ => "online".into(),
    }
}

pub fn likely_country_from_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let table = [
        (
            "HK",
            ["香港", "hong kong", "hongkong", " hk", "hk-", "hkg"].as_slice(),
        ),
        ("TW", ["台湾", "taiwan", " tw", "tw-", "tpe"].as_slice()),
        (
            "JP",
            ["日本", "japan", " jp", "jp-", "tokyo", "osaka"].as_slice(),
        ),
        (
            "SG",
            ["新加坡", "singapore", " sg", "sg-", "sin"].as_slice(),
        ),
        (
            "US",
            [
                "美国",
                "united states",
                " usa",
                " us",
                "us-",
                "los angeles",
                "san jose",
            ]
            .as_slice(),
        ),
        ("KR", ["韩国", "korea", " kr", "kr-", "seoul"].as_slice()),
        (
            "GB",
            ["英国", "united kingdom", " uk", "gb-", "london"].as_slice(),
        ),
        (
            "DE",
            ["德国", "germany", " de", "de-", "frankfurt"].as_slice(),
        ),
    ];
    table
        .iter()
        .find(|(_, keys)| {
            keys.iter()
                .any(|key| lower.contains(&key.to_ascii_lowercase()))
        })
        .map(|(country, _)| (*country).to_string())
}

pub fn parse_host_from_log(payload: &str) -> Option<String> {
    let marker = " --> ";
    let start = payload.find(marker)? + marker.len();
    let tail = &payload[start..];
    let end = tail
        .find(':')
        .or_else(|| tail.find(' '))
        .unwrap_or(tail.len());
    let host = tail[..end].trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

pub fn validate_url(value: &str) -> bool {
    crate::remote::validate_http_url(value)
}

pub fn valid_policy_target(name: &str, available: &std::collections::HashSet<String>) -> bool {
    matches!(name, "DIRECT" | "REJECT" | "GLOBAL" | "PROXY") || available.contains(name)
}

pub fn contains_rule_delimiter_or_control(value: &str) -> bool {
    value.contains(',') || value.chars().any(char::is_control)
}
