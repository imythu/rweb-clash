use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use std::future::Future;
use thiserror::Error;
use tracing::{error, warn};

tokio::task_local! {
    static CURRENT_TRACE_ID: String;
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
    pub trace_id: String,
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct AppError {
    pub status: StatusCode,
    pub code: String,
    pub message: String,
    pub trace_id: Option<String>,
}

impl AppError {
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            trace_id: None,
        }
    }

    pub fn bad_request(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn not_found(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, message)
    }

    pub fn conflict(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    pub fn service_unavailable(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, code, message)
    }

    pub fn internal(message: impl std::fmt::Display) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            message.to_string(),
        )
    }

    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }
}

pub(crate) async fn scope_trace_id<F>(trace_id: String, future: F) -> F::Output
where
    F: Future,
{
    CURRENT_TRACE_ID.scope(trace_id, future).await
}

fn current_trace_id() -> Option<String> {
    CURRENT_TRACE_ID.try_with(Clone::clone).ok()
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let trace_id = self.trace_id.or_else(current_trace_id).unwrap_or_default();
        if self.status.is_server_error() {
            error!(
                status = self.status.as_u16(),
                code = %self.code,
                message = %self.message,
                trace_id = %trace_id,
                "request failed with server error"
            );
        } else {
            warn!(
                status = self.status.as_u16(),
                code = %self.code,
                message = %self.message,
                trace_id = %trace_id,
                "request failed"
            );
        }
        (
            self.status,
            Json(ErrorEnvelope {
                error: ErrorBody {
                    code: self.code,
                    message: self.message,
                    trace_id,
                },
            }),
        )
            .into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        Self::internal(value)
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::internal(value)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::bad_request("invalid_json", value.to_string())
    }
}

impl From<serde_yaml::Error> for AppError {
    fn from(value: serde_yaml::Error) -> Self {
        Self::bad_request("invalid_yaml", value.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(value: reqwest::Error) -> Self {
        if value.is_timeout() || value.is_connect() {
            Self::service_unavailable("network_unreachable", value.to_string())
        } else {
            Self::internal(value)
        }
    }
}
