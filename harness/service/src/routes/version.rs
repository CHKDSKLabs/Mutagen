//! `GET /version` — unauthenticated version probe (FR-4a / ISC-011).

use axum::Json;
use mutagen_core::session::CHAT_PROTOCOL_VERSION;
use utoipa::OpenApi;

use crate::dto::VersionDto;

/// Service binary version. Pulled from this crate's `CARGO_PKG_VERSION` at
/// compile time so it tracks the cargo workspace version.
pub const SERVICE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Harness library version. Same workspace-inherited package version per the
/// slice contract; if the two crates ever decouple, this is the seam to widen.
pub const HARNESS_VERSION: &str = env!("CARGO_PKG_VERSION");

#[utoipa::path(
    get,
    path = "/version",
    tag = "service",
    responses(
        (status = 200, description = "Service, harness, and chat-protocol versions", body = VersionDto),
    ),
)]
pub async fn get_version() -> Json<VersionDto> {
    Json(VersionDto {
        service_version: SERVICE_VERSION.to_string(),
        harness_version: HARNESS_VERSION.to_string(),
        chat_protocol_schema_version: CHAT_PROTOCOL_VERSION.to_string(),
    })
}

#[derive(OpenApi)]
#[openapi(paths(get_version), components(schemas(VersionDto)))]
pub struct VersionApi;
