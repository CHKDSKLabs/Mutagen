//! Canonical error envelope (DSD-624 / INV-E5).
//!
//! Every error response across the service serializes through this struct.
//! Raw error strings never reach the wire — handlers translate domain errors
//! into a stable `code` + a human-readable `message`, then attach a structured
//! `details` blob if and only if the caller can act on it.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorEnvelope {
    /// Stable SCREAMING_SNAKE_CASE machine string. Once shipped, never reused.
    pub code: String,
    /// Human-readable, lowercase, no trailing period.
    pub message: String,
    /// UUIDv7 request correlator echoed in the `X-Request-Id` header.
    pub request_id: String,
    /// Optional structured detail. Omitted on auth failures per INV-A4.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ErrorEnvelope {
    pub fn new(code: impl Into<String>, message: impl Into<String>, request_id: String) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            request_id,
            details: None,
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}
