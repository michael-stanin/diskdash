//! API error type rendered as JSON: `{ "error": "...", "message": "...", "fields": {...} }`.

use std::collections::BTreeMap;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Serialize, TS)]
#[ts(export)]
pub struct ApiErrorBody {
    /// Machine-readable code, e.g. `validation`, `not_found`, `forbidden`.
    pub error: String,
    pub message: String,
    /// Field-level messages for `validation` errors (empty otherwise).
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    NotFound(String),
    #[error("validation failed")]
    Validation(BTreeMap<String, String>),
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Gone(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiError {
    pub fn internal(e: impl std::fmt::Display) -> Self {
        Self::Internal(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            ApiError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation"),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            ApiError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            ApiError::Gone(_) => (StatusCode::GONE, "gone"),
            ApiError::Internal(msg) => {
                tracing::error!(error = %msg, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };
        let fields = match &self {
            ApiError::Validation(f) => f.clone(),
            _ => BTreeMap::new(),
        };
        let body = ApiErrorBody { error: code.into(), message: self.to_string(), fields };
        (status, Json(body)).into_response()
    }
}

/// A JSON error response with an arbitrary status (used by middleware).
pub fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    let body = ApiErrorBody { error: code.into(), message: message.into(), fields: BTreeMap::new() };
    (status, Json(body)).into_response()
}
