//! Workflow write-side DTOs (DDD §3.1 Commands, ISC-005, DSD-322/330/621).
//!
//! Every POST to /projects/{id}/(dispatch-next|slices/{id}/{accept,escalate,
//! finalize,resume,confirm-escalate}) consumes / produces shapes from this
//! module. Domain types stay on the core side of the seam (ISC-010).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// 202 body returned by every Workflow Command (DSD-322: silent success
/// is forbidden — every accepted command echoes back a small envelope so
/// the caller has something to correlate against the WS broadcast).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CommandAcceptedDto {
    /// Stable command name: `dispatch_next` / `accept_review` / `escalate` /
    /// `finalize_slice` / `resume_slice`. Mirrors DDD §3.1 command names.
    pub command: String,
    /// Project this command targets.
    pub project_id: String,
    /// Slice this command targets. `None` for `dispatch_next` because the
    /// next slice is chosen inside the lock and broadcast over the WS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slice_id: Option<String>,
    /// Request-id correlator (same UUIDv7 returned in `X-Request-Id`).
    pub request_id: String,
    /// Wall-clock when the State Update record was appended (DSD-622).
    pub accepted_at: String,
}

/// Empty body shape accepted by accept / finalize / resume. Reserved for
/// future optional notes; defining the type now keeps the OpenAPI surface
/// stable as fields land.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct EmptyCommandBody {
    /// Optional operator note recorded with the State Update. Not used by
    /// the v1 state machine — present so future audit work has a place
    /// without a wire-shape change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Body shape for POST /projects/{id}/slices/{slice_id}/escalate. The
/// confirmation token is minted by `confirm-escalate` (DSD-330).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct EscalateRequest {
    /// Token previously returned by `POST .../confirm-escalate` for this
    /// slice. Single-use, scoped to the slice it was minted against.
    pub confirmation_token: String,
    /// Optional human-readable rationale recorded on the State Update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Response from POST /projects/{id}/slices/{slice_id}/confirm-escalate.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConfirmEscalateDto {
    pub confirmation_token: String,
    pub slice_id: String,
    /// Wall-clock when the token expires (DSD-622). Five minutes from mint.
    pub expires_at: String,
}
