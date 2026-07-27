use crate::error::AppError;
use crate::storage::ProxyItemRecord;
use crate::types::{ManualNodeInput, SUB_DELIMITER};
use crate::util::{content_hash, likely_country_from_name};
use serde_json::{Map, Value};

pub const MANUAL_PROXY_TYPES: &[&str] = &[
    "ss",
    "ssr",
    "vmess",
    "vless",
    "trojan",
    "http",
    "socks5",
    "snell",
    "ssh",
    "hysteria",
    "hysteria2",
    "tuic",
    "wireguard",
    "masque",
    "openvpn",
    "shadowquic",
    "mieru",
    "anytls",
    "sudoku",
    "trusttunnel",
    "tailscale",
    "direct",
    "dns",
    "rematch",
];

const ENDPOINT_PROXY_TYPES: &[&str] = &[
    "ss",
    "ssr",
    "vmess",
    "vless",
    "trojan",
    "http",
    "socks5",
    "snell",
    "ssh",
    "hysteria",
    "hysteria2",
    "tuic",
    "wireguard",
    "masque",
    "openvpn",
    "shadowquic",
    "mieru",
    "anytls",
    "sudoku",
    "trusttunnel",
];

pub fn manual_node_record(input: ManualNodeInput) -> Result<ProxyItemRecord, AppError> {
    let name = input.name.trim();
    if name.is_empty()
        || name.len() > 128
        || name.contains(SUB_DELIMITER)
        || name.chars().any(char::is_control)
    {
        return Err(AppError::bad_request(
            "manual_node_invalid",
            "manual node name must be 1-128 characters and cannot contain reserved delimiters",
        ));
    }
    let mut config = input.config.as_object().cloned().ok_or_else(|| {
        AppError::bad_request(
            "manual_node_invalid",
            "manual node config must be a JSON object",
        )
    })?;
    let protocol = required_string(&config, "type")?.to_ascii_lowercase();
    if !MANUAL_PROXY_TYPES.contains(&protocol.as_str()) {
        return Err(AppError::bad_request(
            "manual_node_unsupported_type",
            format!("unsupported manual proxy type {protocol}"),
        ));
    }
    validate_common_fields(&mut config, &protocol)?;
    validate_protocol_fields(&config, &protocol)?;
    config.insert("name".into(), Value::String(name.to_string()));
    config.insert("type".into(), Value::String(protocol.clone()));
    let raw_json = serde_json::to_string(&config)?;
    Ok(ProxyItemRecord {
        name: name.to_string(),
        kind: "node".into(),
        subscription_id: None,
        display_name: name.to_string(),
        source: "manual".into(),
        builtin: false,
        source_name: Some("手动节点".into()),
        protocol: Some(protocol),
        country: likely_country_from_name(name),
        group_type: None,
        raw_json: Some(raw_json.clone()),
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
        position: 0,
        enabled: true,
    })
}

fn validate_common_fields(config: &mut Map<String, Value>, protocol: &str) -> Result<(), AppError> {
    if !ENDPOINT_PROXY_TYPES.contains(&protocol) {
        return Ok(());
    }
    if protocol == "wireguard" && validate_wireguard_peers(config)? {
        return Ok(());
    }
    let server = required_string(config, "server")?;
    if server.len() > 255 || server.chars().any(char::is_control) {
        return Err(invalid_field("server"));
    }
    if protocol == "mieru" && !config.contains_key("port") {
        let port_range = required_string(config, "port-range")?;
        if !valid_port_range(port_range) {
            return Err(invalid_field("port-range"));
        }
        return Ok(());
    }
    let port = match config.get("port") {
        Some(Value::Number(value)) => value.as_u64(),
        Some(Value::String(value)) => value.trim().parse::<u64>().ok(),
        _ => None,
    }
    .filter(|port| (1..=65_535).contains(port))
    .ok_or_else(|| invalid_field("port"))?;
    config.insert("port".into(), Value::from(port));
    Ok(())
}

fn validate_protocol_fields(config: &Map<String, Value>, protocol: &str) -> Result<(), AppError> {
    let required: &[&[&str]] = match protocol {
        "ss" => &[&["cipher"], &["password"]],
        "ssr" => &[&["cipher"], &["password"], &["protocol"], &["obfs"]],
        "vmess" | "vless" => &[&["uuid"]],
        "trojan" | "anytls" => &[&["password"]],
        "snell" => &[&["psk"]],
        "ssh" => &[&["username"]],
        "hysteria" => &[&["auth-str", "auth"]],
        "hysteria2" => &[&["password", "auth"]],
        "wireguard" => &[&["ip"], &["private-key"]],
        "masque" => &[&["private-key"], &["public-key"]],
        "openvpn" => &[],
        "shadowquic" | "trusttunnel" => &[&["username"], &["password"]],
        "mieru" => &[&["username"], &["password"]],
        "sudoku" => &[&["key"]],
        "http" | "socks5" | "tuic" | "tailscale" | "direct" | "dns" | "rematch" => &[],
        _ => unreachable!("proxy type was checked before field validation"),
    };
    for alternatives in required {
        if !alternatives
            .iter()
            .any(|field| optional_nonempty_string(config, field).is_some())
        {
            return Err(AppError::bad_request(
                "manual_node_missing_field",
                format!(
                    "manual {protocol} node requires {}",
                    alternatives.join(" or ")
                ),
            ));
        }
    }
    if protocol == "wireguard"
        && !has_wireguard_peers(config)
        && optional_nonempty_string(config, "public-key").is_none()
    {
        return Err(AppError::bad_request(
            "manual_node_missing_field",
            "manual wireguard node requires public-key when peers is not configured",
        ));
    }
    if protocol == "tuic" {
        let token = optional_nonempty_string(config, "token").is_some();
        let uuid = optional_nonempty_string(config, "uuid").is_some();
        let password = optional_nonempty_string(config, "password").is_some();
        let uuid_and_password = uuid && password;
        if !token && !uuid_and_password {
            return Err(AppError::bad_request(
                "manual_node_missing_field",
                "manual tuic node requires token or uuid and password",
            ));
        }
        if token && (uuid || password) {
            return Err(AppError::bad_request(
                "manual_node_invalid",
                "manual tuic node cannot mix v4 token with v5 uuid or password",
            ));
        }
    }
    if protocol == "openvpn" {
        validate_openvpn_fields(config)?;
    }
    if protocol == "sudoku"
        && optional_number(config, "padding-min")
            .zip(optional_number(config, "padding-max"))
            .is_some_and(|(minimum, maximum)| maximum < minimum)
    {
        return Err(invalid_field("padding-max"));
    }
    if protocol == "rematch"
        && optional_nonempty_string(config, "target-rematch-name").is_none()
        && optional_nonempty_string(config, "target-sub-rule").is_none()
    {
        return Err(AppError::bad_request(
            "manual_node_missing_field",
            "manual rematch node requires target-rematch-name or target-sub-rule",
        ));
    }
    Ok(())
}

fn validate_openvpn_fields(config: &Map<String, Value>) -> Result<(), AppError> {
    if optional_nonempty_text(config, "ca").is_none() {
        return Err(AppError::bad_request(
            "manual_node_missing_field",
            "manual openvpn node requires ca",
        ));
    }

    let username = optional_nonempty_string(config, "username").is_some();
    let password = optional_nonempty_string(config, "password").is_some();
    let certificate = optional_nonempty_text(config, "cert").is_some();
    let private_key = optional_nonempty_text(config, "key").is_some();
    if username != password || certificate != private_key {
        return Err(AppError::bad_request(
            "manual_node_invalid",
            "manual openvpn credentials must be configured in complete pairs",
        ));
    }
    if !(username && password || certificate && private_key) {
        return Err(AppError::bad_request(
            "manual_node_missing_field",
            "manual openvpn node requires username and password or cert and key",
        ));
    }

    let tls_key_count = ["tls-auth", "tls-crypt", "tls-crypt-v2"]
        .into_iter()
        .filter(|field| optional_nonempty_text(config, field).is_some())
        .count();
    if tls_key_count > 1 {
        return Err(AppError::bad_request(
            "manual_node_invalid",
            "manual openvpn node accepts only one of tls-auth, tls-crypt, or tls-crypt-v2",
        ));
    }
    Ok(())
}

fn has_wireguard_peers(config: &Map<String, Value>) -> bool {
    config
        .get("peers")
        .and_then(Value::as_array)
        .is_some_and(|peers| !peers.is_empty())
}

fn validate_wireguard_peers(config: &mut Map<String, Value>) -> Result<bool, AppError> {
    let Some(value) = config.get_mut("peers") else {
        return Ok(false);
    };
    let peers = value.as_array_mut().ok_or_else(|| invalid_field("peers"))?;
    if peers.is_empty() {
        return Err(invalid_field("peers"));
    }
    for peer in peers {
        let peer = peer.as_object_mut().ok_or_else(|| invalid_field("peers"))?;
        let server = required_string(peer, "server")?;
        if server.len() > 255 || server.chars().any(char::is_control) {
            return Err(invalid_field("peers.server"));
        }
        required_string(peer, "public-key")?;
        let port = match peer.get("port") {
            Some(Value::Number(value)) => value.as_u64(),
            Some(Value::String(value)) => value.trim().parse::<u64>().ok(),
            _ => None,
        }
        .filter(|port| (1..=65_535).contains(port))
        .ok_or_else(|| invalid_field("peers.port"))?;
        peer.insert("port".into(), Value::from(port));
    }
    Ok(true)
}

fn required_string<'a>(config: &'a Map<String, Value>, key: &str) -> Result<&'a str, AppError> {
    optional_nonempty_string(config, key).ok_or_else(|| {
        AppError::bad_request(
            "manual_node_missing_field",
            format!("manual node requires {key}"),
        )
    })
}

fn optional_nonempty_string<'a>(config: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    config
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.chars().any(char::is_control))
}

fn optional_nonempty_text<'a>(config: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    config
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| {
            !value.is_empty()
                && !value.chars().any(|character| {
                    character.is_control() && !matches!(character, '\r' | '\n' | '\t')
                })
        })
}

fn optional_number(config: &Map<String, Value>, key: &str) -> Option<f64> {
    match config.get(key) {
        Some(Value::Number(value)) => value.as_f64(),
        Some(Value::String(value)) => value.trim().parse().ok(),
        _ => None,
    }
}

fn valid_port_range(value: &str) -> bool {
    value.split(',').all(|part| {
        let part = part.trim();
        if let Some((start, end)) = part.split_once('-') {
            let Ok(start) = start.trim().parse::<u16>() else {
                return false;
            };
            let Ok(end) = end.trim().parse::<u16>() else {
                return false;
            };
            start > 0 && start <= end
        } else {
            part.parse::<u16>().is_ok_and(|port| port > 0)
        }
    })
}

fn invalid_field(field: &str) -> AppError {
    AppError::bad_request(
        "manual_node_invalid",
        format!("manual node contains an invalid {field}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builds_a_normalized_manual_vless_node() {
        let record = manual_node_record(ManualNodeInput {
            name: "Private VLESS".into(),
            config: json!({
                "type": "VLESS",
                "server": "example.com",
                "port": "443",
                "uuid": "00000000-0000-0000-0000-000000000000",
                "tls": true
            }),
        })
        .expect("valid VLESS node");

        assert_eq!(record.protocol.as_deref(), Some("vless"));
        let config: Value = serde_json::from_str(record.raw_json.as_deref().unwrap()).unwrap();
        assert_eq!(config["name"], "Private VLESS");
        assert_eq!(config["port"], 443);
    }

    #[test]
    fn rejects_missing_protocol_credentials() {
        let error = manual_node_record(ManualNodeInput {
            name: "Broken".into(),
            config: json!({ "type": "trojan", "server": "example.com", "port": 443 }),
        })
        .expect_err("missing password must fail");

        assert_eq!(error.code, "manual_node_missing_field");
    }

    #[test]
    fn accepts_serverless_dns_and_tailscale_nodes() {
        for (name, config) in [
            ("DNS Out", json!({ "type": "dns" })),
            (
                "Tailnet",
                json!({ "type": "tailscale", "hostname": "mihomo" }),
            ),
        ] {
            let record = manual_node_record(ManualNodeInput {
                name: name.into(),
                config,
            })
            .expect("serverless node must not require an endpoint");
            assert_eq!(record.display_name, name);
        }
    }

    #[test]
    fn accepts_both_tuic_credential_modes() {
        for credentials in [
            json!({ "token": "secret" }),
            json!({ "uuid": "00000000-0000-0000-0000-000000000000", "password": "secret" }),
        ] {
            let mut config = json!({
                "type": "tuic",
                "server": "example.com",
                "port": 443
            });
            config
                .as_object_mut()
                .unwrap()
                .extend(credentials.as_object().unwrap().clone());
            manual_node_record(ManualNodeInput {
                name: "TUIC".into(),
                config,
            })
            .expect("valid TUIC credentials");
        }
    }

    #[test]
    fn accepts_and_normalizes_wireguard_peers_without_a_top_level_endpoint() {
        let record = manual_node_record(ManualNodeInput {
            name: "WireGuard Mesh".into(),
            config: json!({
                "type": "wireguard",
                "ip": "172.16.0.2",
                "private-key": "private-key",
                "peers": [{
                    "server": "wg.example.com",
                    "port": "51820",
                    "public-key": "public-key",
                    "allowed-ips": ["0.0.0.0/0"]
                }]
            }),
        })
        .expect("wireguard peers should replace the top-level endpoint");

        let config: Value = serde_json::from_str(record.raw_json.as_deref().unwrap()).unwrap();
        assert_eq!(config["peers"][0]["port"], 51_820);
        assert!(config.get("server").is_none());
        assert!(config.get("public-key").is_none());
    }

    #[test]
    fn rejects_a_wireguard_peer_without_a_public_key() {
        let error = manual_node_record(ManualNodeInput {
            name: "Broken WireGuard".into(),
            config: json!({
                "type": "wireguard",
                "ip": "172.16.0.2",
                "private-key": "private-key",
                "peers": [{ "server": "wg.example.com", "port": 51820 }]
            }),
        })
        .expect_err("each wireguard peer needs its own public key");

        assert_eq!(error.code, "manual_node_missing_field");
    }

    #[test]
    fn accepts_multiline_openvpn_pem_with_password_authentication() {
        let record = manual_node_record(ManualNodeInput {
            name: "OpenVPN".into(),
            config: json!({
                "type": "openvpn",
                "server": "vpn.example.com",
                "port": 1194,
                "username": "alice",
                "password": "secret",
                "ca": "-----BEGIN CERTIFICATE-----\nZmFrZQ==\n-----END CERTIFICATE-----"
            }),
        })
        .expect("multiline PEM should be accepted");

        let config: Value = serde_json::from_str(record.raw_json.as_deref().unwrap()).unwrap();
        assert!(config["ca"].as_str().unwrap().contains('\n'));
    }

    #[test]
    fn rejects_incomplete_or_conflicting_openvpn_credentials() {
        for config in [
            json!({
                "type": "openvpn",
                "server": "vpn.example.com",
                "port": 1194,
                "username": "alice",
                "ca": "certificate"
            }),
            json!({
                "type": "openvpn",
                "server": "vpn.example.com",
                "port": 1194,
                "username": "alice",
                "password": "secret",
                "ca": "certificate",
                "tls-auth": "auth-key",
                "tls-crypt": "crypt-key"
            }),
        ] {
            let error = manual_node_record(ManualNodeInput {
                name: "Broken OpenVPN".into(),
                config,
            })
            .expect_err("invalid OpenVPN credential combinations must fail");
            assert_eq!(error.code, "manual_node_invalid");
        }
    }

    #[test]
    fn rejects_mixed_tuic_versions_and_inverted_sudoku_padding() {
        for config in [
            json!({
                "type": "tuic",
                "server": "tuic.example.com",
                "port": 443,
                "token": "v4-token",
                "uuid": "00000000-0000-0000-0000-000000000000",
                "password": "v5-password"
            }),
            json!({
                "type": "sudoku",
                "server": "sudoku.example.com",
                "port": 443,
                "key": "secret",
                "padding-min": 80,
                "padding-max": 20
            }),
        ] {
            let error = manual_node_record(ManualNodeInput {
                name: "Broken Protocol".into(),
                config,
            })
            .expect_err("cross-field protocol constraints must fail");
            assert_eq!(error.code, "manual_node_invalid");
        }
    }
}
