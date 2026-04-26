use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use shared_types::{ConnectionSummary, ProxyDelayResponse, ProxyGroupSummary, ProxyHistoryItem};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ControllerClient {
    base_url: String,
    secret: Option<String>,
    client: reqwest::Client,
}

impl ControllerClient {
    pub fn new(addr: impl AsRef<str>, secret: Option<String>) -> Self {
        let addr = addr.as_ref();
        let base_url = if addr.starts_with("http://") || addr.starts_with("https://") {
            addr.to_string()
        } else {
            format!("http://{addr}")
        };

        Self {
            base_url,
            secret,
            client: reqwest::Client::new(),
        }
    }

    pub async fn version(&self) -> Result<Option<String>, ControllerError> {
        #[derive(Deserialize)]
        struct VersionResponse {
            version: Option<String>,
        }

        let payload: VersionResponse = self
            .request_json(Method::GET, "/version", None::<()>)
            .await?;
        Ok(payload.version)
    }

    pub async fn reload_config(&self, path: &str) -> Result<(), ControllerError> {
        #[derive(serde::Serialize)]
        struct ReloadBody<'a> {
            path: &'a str,
            force: bool,
        }

        self.request_empty(
            Method::PUT,
            "/configs",
            Some(ReloadBody { path, force: true }),
        )
        .await
    }

    pub async fn proxies(&self) -> Result<Vec<ProxyGroupSummary>, ControllerError> {
        let payload: ProxiesResponse = self
            .request_json(Method::GET, "/proxies", None::<()>)
            .await?;
        let mut groups = payload
            .proxies
            .into_iter()
            .filter_map(|(name, proxy)| {
                proxy.all.map(|all| ProxyGroupSummary {
                    name,
                    kind: proxy.kind,
                    now: proxy.now,
                    all,
                    history: proxy
                        .history
                        .unwrap_or_default()
                        .into_iter()
                        .map(|item| ProxyHistoryItem {
                            time: item.time,
                            delay: item.delay,
                        })
                        .collect(),
                })
            })
            .collect::<Vec<_>>();

        groups.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(groups)
    }

    pub async fn select_proxy(&self, group: &str, name: &str) -> Result<(), ControllerError> {
        #[derive(serde::Serialize)]
        struct SelectBody<'a> {
            name: &'a str,
        }

        let encoded = urlencoding::encode(group);
        self.request_empty(
            Method::PUT,
            &format!("/proxies/{encoded}"),
            Some(SelectBody { name }),
        )
        .await
    }

    pub async fn proxy_delay(
        &self,
        name: &str,
        url: &str,
        timeout_ms: u64,
    ) -> Result<ProxyDelayResponse, ControllerError> {
        #[derive(Deserialize)]
        struct DelayResponse {
            delay: Option<u64>,
        }

        let encoded = urlencoding::encode(name);
        let test_url = urlencoding::encode(url);
        let payload: DelayResponse = self
            .request_json(
                Method::GET,
                &format!("/proxies/{encoded}/delay?timeout={timeout_ms}&url={test_url}"),
                None::<()>,
            )
            .await?;

        Ok(ProxyDelayResponse {
            name: name.to_string(),
            delay: payload.delay,
        })
    }

    pub async fn group_delay(
        &self,
        name: &str,
        url: &str,
        timeout_ms: u64,
    ) -> Result<Vec<ProxyDelayResponse>, ControllerError> {
        let encoded = urlencoding::encode(name);
        let test_url = urlencoding::encode(url);
        let payload: serde_json::Value = self
            .request_json(
                Method::GET,
                &format!("/group/{encoded}/delay?timeout={timeout_ms}&url={test_url}"),
                None::<()>,
            )
            .await?;

        let mut delays = Vec::new();
        collect_delay_entries(&payload, &mut delays);
        Ok(delays)
    }

    pub async fn connections(&self) -> Result<Vec<ConnectionSummary>, ControllerError> {
        let payload: ConnectionsResponse = self
            .request_json(Method::GET, "/connections", None::<()>)
            .await?;

        Ok(payload
            .connections
            .into_iter()
            .map(|connection| ConnectionSummary {
                id: connection.id,
                host: connection.metadata.host,
                destination_ip: connection.metadata.destination_ip,
                destination_port: connection.metadata.destination_port,
                network: connection.metadata.network,
                r#type: connection.metadata.kind,
                process: connection.metadata.process,
                rule: connection.rule,
                rule_payload: connection.rule_payload,
                chains: connection.chains,
                upload: connection.upload,
                download: connection.download,
                start: connection.start,
            })
            .collect())
    }

    pub async fn close_connection(&self, connection_id: &str) -> Result<(), ControllerError> {
        let encoded = urlencoding::encode(connection_id);
        self.request_empty(
            Method::DELETE,
            &format!("/connections/{encoded}"),
            None::<()>,
        )
        .await
    }

    async fn request_json<T, B>(
        &self,
        method: Method,
        path: &str,
        body: Option<B>,
    ) -> Result<T, ControllerError>
    where
        T: DeserializeOwned,
        B: serde::Serialize,
    {
        let request = self.request_builder(method, path, body);
        let response = request.send().await.map_err(ControllerError::Http)?;
        let response = response.error_for_status().map_err(ControllerError::Http)?;
        response.json::<T>().await.map_err(ControllerError::Decode)
    }

    async fn request_empty<B>(
        &self,
        method: Method,
        path: &str,
        body: Option<B>,
    ) -> Result<(), ControllerError>
    where
        B: serde::Serialize,
    {
        let response = self
            .request_builder(method, path, body)
            .send()
            .await
            .map_err(ControllerError::Http)?;

        if response.status() == StatusCode::NO_CONTENT || response.status().is_success() {
            Ok(())
        } else {
            Err(ControllerError::UnexpectedStatus(response.status()))
        }
    }

    fn request_builder<B>(
        &self,
        method: Method,
        path: &str,
        body: Option<B>,
    ) -> reqwest::RequestBuilder
    where
        B: serde::Serialize,
    {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path));
        if let Some(secret) = &self.secret {
            if !secret.is_empty() {
                request = request.bearer_auth(secret);
            }
        }
        if let Some(body) = body {
            request.json(&body)
        } else {
            request
        }
    }
}

fn collect_delay_entries(value: &serde_json::Value, delays: &mut Vec<ProxyDelayResponse>) {
    let Some(object) = value.as_object() else {
        return;
    };

    if let (Some(name), Some(delay)) = (
        object.get("name").and_then(serde_json::Value::as_str),
        object.get("delay").and_then(serde_json::Value::as_u64),
    ) {
        delays.push(ProxyDelayResponse {
            name: name.to_string(),
            delay: Some(delay),
        });
        return;
    }

    for (name, value) in object {
        if let Some(delay) = value.as_u64() {
            delays.push(ProxyDelayResponse {
                name: name.clone(),
                delay: Some(delay),
            });
        } else {
            collect_delay_entries(value, delays);
        }
    }
}

#[derive(Debug, Error)]
pub enum ControllerError {
    #[error("controller http error: {0}")]
    Http(reqwest::Error),
    #[error("controller response decode error: {0}")]
    Decode(reqwest::Error),
    #[error("controller returned unexpected status {0}")]
    UnexpectedStatus(StatusCode),
}

#[derive(Debug, Deserialize)]
struct ProxiesResponse {
    proxies: HashMap<String, ControllerProxy>,
}

#[derive(Debug, Deserialize)]
struct ControllerProxy {
    #[serde(rename = "type")]
    kind: Option<String>,
    now: Option<String>,
    all: Option<Vec<String>>,
    history: Option<Vec<ControllerHistory>>,
}

#[derive(Debug, Deserialize)]
struct ControllerHistory {
    time: Option<String>,
    delay: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ConnectionsResponse {
    #[serde(default, deserialize_with = "null_to_default")]
    connections: Vec<ControllerConnection>,
}

#[derive(Debug, Deserialize)]
struct ControllerConnection {
    id: String,
    #[serde(default)]
    metadata: ControllerMetadata,
    #[serde(default)]
    upload: u64,
    #[serde(default)]
    download: u64,
    start: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    chains: Vec<String>,
    rule: Option<String>,
    #[serde(rename = "rulePayload")]
    rule_payload: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ControllerMetadata {
    host: Option<String>,
    #[serde(rename = "destinationIP")]
    destination_ip: Option<String>,
    #[serde(rename = "destinationPort")]
    destination_port: Option<String>,
    network: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    process: Option<String>,
}

fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}
