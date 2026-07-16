use crate::error::AppError;
use crate::types::{ConnectionResponse, DelayResponse, TrafficResponse};
use axum::http::{Method, StatusCode};
use bytes::Bytes;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct ControllerClient {
    base_url: String,
    secret: Option<String>,
    client: reqwest::Client,
}

impl ControllerClient {
    pub fn new(addr: String, secret: Option<String>) -> Result<Self, AppError> {
        let base_url = if addr.starts_with("http://") || addr.starts_with("https://") {
            addr.trim_end_matches('/').to_string()
        } else {
            format!("http://{}", addr.trim_end_matches('/'))
        };
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .no_proxy()
            .build()
            .map_err(AppError::internal)?;
        Ok(Self {
            base_url,
            secret,
            client,
        })
    }

    pub async fn reload_config(&self, path: &str) -> Result<(), AppError> {
        info!(path, "reloading mihomo config through controller");
        self.request_empty(
            Method::PUT,
            "/configs",
            Some(json!({
                "path": path,
                "payload": "",
            })),
        )
        .await
    }

    pub async fn select_proxy(&self, group: &str, name: &str) -> Result<(), AppError> {
        info!(group, name, "selecting proxy through controller");
        self.request_empty(
            Method::PUT,
            &format!("/proxies/{}", urlencoding::encode(group)),
            Some(json!({ "name": name })),
        )
        .await
    }

    pub async fn proxy_delay(
        &self,
        name: &str,
        test_url: &str,
        timeout_ms: u64,
    ) -> Result<DelayResponse, AppError> {
        info!(name, timeout_ms, "testing proxy delay through controller");
        let path = format!(
            "/proxies/{}/delay?timeout={}&url={}",
            urlencoding::encode(name),
            timeout_ms,
            urlencoding::encode(test_url)
        );
        let result: ControllerDelay = self.request_json(Method::GET, &path, None::<()>).await?;
        Ok(DelayResponse {
            name: name.to_string(),
            delay: result.delay.unwrap_or_default(),
        })
    }

    pub async fn group_delay(
        &self,
        name: &str,
        test_url: &str,
        timeout_ms: u64,
    ) -> Result<Vec<DelayResponse>, AppError> {
        info!(
            name,
            timeout_ms, "testing proxy group delay through controller"
        );
        let path = format!(
            "/group/{}/delay?timeout={}&url={}",
            urlencoding::encode(name),
            timeout_ms,
            urlencoding::encode(test_url)
        );
        let result: std::collections::HashMap<String, i64> =
            self.request_json(Method::GET, &path, None::<()>).await?;
        Ok(result
            .into_iter()
            .map(|(name, delay)| DelayResponse { name, delay })
            .collect())
    }

    pub async fn traffic_sample(&self) -> Result<TrafficResponse, AppError> {
        let url = self.url("/traffic");
        let mut request = self.client.get(url);
        if let Some(secret) = self.secret.as_deref().filter(|secret| !secret.is_empty()) {
            request = request.bearer_auth(secret);
        }
        let response = request.send().await.map_err(AppError::from)?;
        if !response.status().is_success() {
            return Err(AppError::new(
                response.status(),
                "controller_unexpected_status",
                format!("controller returned {}", response.status()),
            ));
        }
        let mut stream = response.bytes_stream();
        let chunk = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .map_err(|_| {
                AppError::new(
                    StatusCode::GATEWAY_TIMEOUT,
                    "controller_timeout",
                    "controller traffic stream timed out",
                )
            })?
            .transpose()
            .map_err(AppError::from)?
            .unwrap_or_else(Bytes::new);
        parse_traffic_chunk(&chunk)
    }

    pub async fn connections(&self) -> Result<Vec<ConnectionResponse>, AppError> {
        let payload: ControllerConnections = self
            .request_json(Method::GET, "/connections", None::<()>)
            .await?;
        Ok(payload
            .connections
            .into_iter()
            .map(|conn| ConnectionResponse {
                id: conn.id,
                domain: conn.metadata.host.or(conn.metadata.destination_ip),
                rule: conn.rule,
                policy: conn.chains.last().cloned(),
                speed: "实时".into(),
            })
            .collect())
    }

    pub async fn close_connection(&self, id: &str) -> Result<(), AppError> {
        info!(connection_id = %id, "closing controller connection");
        self.request_empty(
            Method::DELETE,
            &format!("/connections/{}", urlencoding::encode(id)),
            None::<()>,
        )
        .await
    }

    pub async fn flush_dns(&self) -> Result<(), AppError> {
        info!("flushing controller dns cache");
        self.request_empty(Method::POST, "/cache/fakeip/flush", None::<()>)
            .await
    }

    async fn request_json<T, B>(
        &self,
        method: Method,
        path: &str,
        body: Option<B>,
    ) -> Result<T, AppError>
    where
        T: DeserializeOwned,
        B: serde::Serialize,
    {
        let response = self.request(method, path, body).await?;
        let status = response.status();
        let text = response.text().await.map_err(AppError::from)?;
        if status.is_success() {
            serde_json::from_str(&text).map_err(|err| {
                warn!(status = status.as_u16(), body = %text, error = %err, "controller response decode failed");
                AppError::new(StatusCode::BAD_GATEWAY, "controller_decode_failed", err.to_string())
            })
        } else {
            Err(AppError::new(
                status,
                "controller_unexpected_status",
                if text.is_empty() {
                    format!("controller returned {status}")
                } else {
                    text
                },
            ))
        }
    }

    async fn request_empty<B>(
        &self,
        method: Method,
        path: &str,
        body: Option<B>,
    ) -> Result<(), AppError>
    where
        B: serde::Serialize,
    {
        let response = self.request(method, path, body).await?;
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            Err(AppError::new(
                status,
                "controller_unexpected_status",
                if text.is_empty() {
                    format!("controller returned {status}")
                } else {
                    text
                },
            ))
        }
    }

    async fn request<B>(
        &self,
        method: Method,
        path: &str,
        body: Option<B>,
    ) -> Result<reqwest::Response, AppError>
    where
        B: serde::Serialize,
    {
        let mut request = self.client.request(method, self.url(path));
        if let Some(secret) = self.secret.as_deref().filter(|secret| !secret.is_empty()) {
            request = request.bearer_auth(secret);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        request.send().await.map_err(AppError::from)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

#[derive(Debug, Deserialize)]
struct ControllerDelay {
    #[serde(default)]
    delay: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ControllerConnections {
    connections: Vec<ControllerConnection>,
}

#[derive(Debug, Deserialize)]
struct ControllerConnection {
    id: String,
    metadata: ControllerMetadata,
    rule: Option<String>,
    #[serde(default)]
    chains: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ControllerMetadata {
    host: Option<String>,
    #[serde(rename = "destinationIP")]
    destination_ip: Option<String>,
}

fn parse_traffic_chunk(chunk: &[u8]) -> Result<TrafficResponse, AppError> {
    if chunk.is_empty() {
        return Ok(TrafficResponse { up: 0, down: 0 });
    }
    serde_json::from_slice::<TrafficResponse>(chunk).map_err(|err| {
        AppError::new(
            StatusCode::BAD_GATEWAY,
            "controller_decode_failed",
            err.to_string(),
        )
    })
}
