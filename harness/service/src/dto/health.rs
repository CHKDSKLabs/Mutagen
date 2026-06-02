//! Health + Version DTOs (DDD §3.4 Value Objects).
//!
//! `HealthDto` answers `GET /health`; `VersionDto` answers `GET /version`.
//! Both endpoints are on the unauthenticated allowlist (ISC-011) so monitoring
//! and downstream GUI clients can probe feature availability before they
//! commit the shared secret.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthDto {
    /// `ok` | `degraded` | `down`. Stringly-typed on the wire by design —
    /// downstream clients pattern-match, they don't enumerate.
    pub status: String,
    /// The same string `VersionDto::service_version` reports.
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct VersionDto {
    /// Service binary version (Cargo package version of `mutagen-service`).
    pub service_version: String,
    /// Harness library version linked into this service process.
    pub harness_version: String,
    /// Wire-protocol schema version advertised by the chat WebSocket.
    pub chat_protocol_schema_version: String,
}
