//! Workflow read-side DTOs (DDD §3.1, ISC-006/007/010, DSD-621/625).
//!
//! Status, slice queue, and state-log shapes for the read endpoints. Every
//! field is snake_case on the wire (DSD-621) and no domain type from
//! mutagen-core leaks through — handlers translate at the seam per ISC-010.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
pub struct SliceCountsDto {
    pub pending: u32,
    pub in_progress: u32,
    pub blocked_retry: u32,
    pub completed: u32,
    pub escalated: u32,
    pub refused: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StatusDto {
    pub project_id: String,
    pub pipeline_mode: String,
    /// Slice currently held in `.mutagen/state/active-slice.json`, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_slice_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_stage: Option<String>,
    pub slice_counts: SliceCountsDto,
    pub total_slices: u32,
    /// Wall-clock of the last State Update record appended to the log (DSD-622).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_state_update_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SliceDto {
    pub id: String,
    pub title: String,
    pub status: String,
    pub layer: u32,
    pub bounded_context: String,
    pub author_agent: String,
    pub attempts: u32,
    pub target_loc: u32,
    pub objective: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SliceQueueDto {
    pub project_id: String,
    pub pipeline_mode: String,
    pub generated_at: String,
    pub slices: Vec<SliceDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct OriginDto {
    /// `cli` | `service`.
    pub kind: String,
    /// `cli:<pid>` → "<pid>" as a string; `service:<session_id>` → session id.
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StateUpdateDto {
    pub slice_id: String,
    pub event: String,
    pub at: String,
    /// Absent only for legacy pre-0.4.0 records (MD-4). Post-0.4.0 records
    /// always carry this — replay fails closed otherwise (ISC-007).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<OriginDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StateLogPageDto {
    pub items: Vec<StateUpdateDto>,
    /// Opaque cursor (DSD-625) — base64url of the next byte offset. Absent at tail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}
