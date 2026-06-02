//! `GET /projects/{id}/status`, `/slices`, `/state-log` — FR-1, FR-15,
//! DDD §3.1 read queries, ISC-006/007/010/013, DSD-300/301/621/625/627.
//!
//! Read side only. No Project Lock acquisition (DDD §3.1 strong-consistency
//! reads). Domain types from mutagen-core are translated to DTOs at the seam.

use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use axum::{
    Extension, Json, Router,
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use mutagen_core::project::registry::LookupError;
use mutagen_core::queue::SliceQueue;
use mutagen_core::state::{ActiveSliceState, Stage};
use mutagen_core::validation::load_queue_file;
use mutagen_core::workflow::origin::Origin;
use mutagen_core::workflow::state_update::log_path;
use serde::Deserialize;
use serde_json::{Value, json};
use utoipa::OpenApi;

use crate::dto::{
    ErrorEnvelope, OriginDto, SliceCountsDto, SliceDto, SliceQueueDto, StateLogPageDto,
    StateUpdateDto, StatusDto,
};
use crate::observability::RequestId;
use crate::routes::projects::ProjectsState;

pub fn router(state: ProjectsState) -> Router {
    Router::new()
        .route("/projects/:project_id/status", get(get_status))
        .route("/projects/:project_id/slices", get(get_slices))
        .route("/projects/:project_id/state-log", get(get_state_log))
        .with_state(state)
}

fn resolve_root(state: &ProjectsState, project_id: &str) -> Result<PathBuf, ()> {
    let guard = match state.registry.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    match guard.lookup(project_id) {
        Ok(entry) => Ok(entry.root.clone()),
        Err(LookupError::NotFound) => Err(()),
    }
}

fn not_found(rid: String) -> Response {
    let envelope = ErrorEnvelope::new("PROJECT_NOT_FOUND", "no project with that id", rid);
    (StatusCode::NOT_FOUND, Json(envelope)).into_response()
}

fn internal(rid: String, code: &str, message: &str, reason: String) -> Response {
    let envelope = ErrorEnvelope::new(code, message, rid).with_details(json!({ "reason": reason }));
    (StatusCode::INTERNAL_SERVER_ERROR, Json(envelope)).into_response()
}

fn slice_status_str(s: mutagen_core::queue::SliceStatus) -> &'static str {
    use mutagen_core::queue::SliceStatus::*;
    match s {
        Pending => "pending",
        InProgress => "in_progress",
        BlockedRetry => "blocked_retry",
        Completed => "completed",
        Escalated => "escalated",
        Refused => "refused",
        FinalizationFailed => "finalization_failed",
    }
}

fn stage_str(s: Stage) -> &'static str {
    match s {
        Stage::Author => "author",
        Stage::StructuralCheck => "structural_check",
        Stage::Review => "review",
        Stage::StateRecord => "state_record",
    }
}

fn pipeline_mode_str(m: mutagen_core::config::PipelineMode) -> &'static str {
    use mutagen_core::config::PipelineMode::*;
    match m {
        Full => "full",
        Lightweight => "lightweight",
    }
}

fn count(queue: &SliceQueue) -> SliceCountsDto {
    let mut c = SliceCountsDto::default();
    for s in &queue.slices {
        use mutagen_core::queue::SliceStatus::*;
        match s.status {
            Pending => c.pending += 1,
            InProgress => c.in_progress += 1,
            BlockedRetry => c.blocked_retry += 1,
            Completed => c.completed += 1,
            Escalated => c.escalated += 1,
            Refused => c.refused += 1,
            // Bucketed with refused for the SliceCountsDto wire shape; both
            // states mean "human attention needed before this slice moves".
            FinalizationFailed => c.refused += 1,
        }
    }
    c
}

fn read_queue(root: &Path) -> Option<SliceQueue> {
    let p = root.join("slices/queue.json");
    if !p.exists() {
        return None;
    }
    load_queue_file(&p).ok()
}

fn read_active(root: &Path) -> Option<ActiveSliceState> {
    let p = root.join(".mutagen/state/active-slice.json");
    if !p.exists() {
        return None;
    }
    let raw = fs::read_to_string(&p).ok()?;
    serde_json::from_str(&raw).ok()
}

fn last_log_at(root: &Path) -> Option<String> {
    let p = log_path(root);
    let raw = fs::read_to_string(&p).ok()?;
    let line = raw.lines().rfind(|l| !l.trim().is_empty())?;
    let v: Value = serde_json::from_str(line).ok()?;
    v.get("at").and_then(Value::as_str).map(str::to_string)
}

#[utoipa::path(
    get,
    path = "/projects/{project_id}/status",
    tag = "workflow",
    params(("project_id" = String, Path, description = "Project UUIDv7")),
    responses(
        (status = 200, description = "Workflow status snapshot", body = StatusDto),
        (status = 404, description = "Unknown project_id", body = ErrorEnvelope),
    ),
)]
pub async fn get_status(
    State(state): State<ProjectsState>,
    AxumPath(project_id): AxumPath<String>,
    Extension(rid): Extension<RequestId>,
) -> Response {
    let rid = rid.0;
    let Ok(root) = resolve_root(&state, &project_id) else {
        return not_found(rid);
    };

    let queue = read_queue(&root);
    let active = read_active(&root);
    let counts = queue.as_ref().map(count).unwrap_or_default();
    let total = queue.as_ref().map(|q| q.slices.len() as u32).unwrap_or(0);
    let pipeline_mode = queue
        .as_ref()
        .map(|q| pipeline_mode_str(q.pipeline_mode).to_string())
        .unwrap_or_else(|| "full".to_string());

    let body = StatusDto {
        project_id,
        pipeline_mode,
        active_slice_id: active.as_ref().map(|a| a.slice_id.clone()),
        active_stage: active.as_ref().map(|a| stage_str(a.stage).to_string()),
        slice_counts: counts,
        total_slices: total,
        last_state_update_at: last_log_at(&root),
    };
    (StatusCode::OK, Json(body)).into_response()
}

#[utoipa::path(
    get,
    path = "/projects/{project_id}/slices",
    tag = "workflow",
    params(("project_id" = String, Path, description = "Project UUIDv7")),
    responses(
        (status = 200, description = "Slice queue snapshot", body = SliceQueueDto),
        (status = 404, description = "Unknown project_id", body = ErrorEnvelope),
    ),
)]
pub async fn get_slices(
    State(state): State<ProjectsState>,
    AxumPath(project_id): AxumPath<String>,
    Extension(rid): Extension<RequestId>,
) -> Response {
    let rid = rid.0;
    let Ok(root) = resolve_root(&state, &project_id) else {
        return not_found(rid);
    };

    let Some(queue) = read_queue(&root) else {
        return (
            StatusCode::OK,
            Json(SliceQueueDto {
                project_id,
                pipeline_mode: "full".to_string(),
                generated_at: String::new(),
                slices: Vec::new(),
            }),
        )
            .into_response();
    };

    let slices = queue
        .slices
        .iter()
        .map(|s| SliceDto {
            id: s.id.clone(),
            title: s.title.clone(),
            status: slice_status_str(s.status).to_string(),
            layer: s.layer,
            bounded_context: s.bounded_context.clone(),
            author_agent: s.author_agent.clone(),
            attempts: s.attempts,
            target_loc: s.target_loc,
            objective: s.objective.clone(),
        })
        .collect();

    let body = SliceQueueDto {
        project_id,
        pipeline_mode: pipeline_mode_str(queue.pipeline_mode).to_string(),
        generated_at: queue.generated_at.clone(),
        slices,
    };
    (StatusCode::OK, Json(body)).into_response()
}

#[derive(Debug, Deserialize)]
pub struct StateLogQuery {
    /// Opaque cursor token (DSD-625). Decodes to a byte offset into the log.
    pub cursor: Option<String>,
    /// Soft cap on records returned. Server may return fewer at EOF.
    pub limit: Option<u32>,
}

const DEFAULT_LIMIT: u32 = 100;
const MAX_LIMIT: u32 = 1000;

fn decode_cursor(c: &str) -> Option<u64> {
    // The cursor is a hex-encoded byte offset. We chose hex over base64 to
    // keep the token URL-safe without pulling in a base64 dep.
    u64::from_str_radix(c, 16).ok()
}

fn encode_cursor(offset: u64) -> String {
    format!("{offset:x}")
}

fn origin_to_dto(o: &Origin) -> OriginDto {
    match o {
        Origin::Cli { pid } => OriginDto {
            kind: "cli".to_string(),
            id: pid.to_string(),
        },
        Origin::Service { session_id } => OriginDto {
            kind: "service".to_string(),
            id: session_id.clone(),
        },
    }
}

#[utoipa::path(
    get,
    path = "/projects/{project_id}/state-log",
    tag = "workflow",
    params(
        ("project_id" = String, Path, description = "Project UUIDv7"),
        ("cursor" = Option<String>, Query, description = "Opaque pagination cursor (DSD-625)"),
        ("limit" = Option<u32>, Query, description = "Soft cap on records; default 100, max 1000"),
    ),
    responses(
        (status = 200, description = "State update log page", body = StateLogPageDto),
        (status = 404, description = "Unknown project_id", body = ErrorEnvelope),
    ),
)]
pub async fn get_state_log(
    State(state): State<ProjectsState>,
    AxumPath(project_id): AxumPath<String>,
    Query(q): Query<StateLogQuery>,
    Extension(rid): Extension<RequestId>,
) -> Response {
    let rid = rid.0;
    let Ok(root) = resolve_root(&state, &project_id) else {
        return not_found(rid);
    };

    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT) as usize;
    // Opaque cursors are server-minted (DSD-625). A gibberish cursor means
    // tampering, transport mangling, or a client bug — never silently rewind
    // to start=0, that hands the caller a duplicate first page.
    let start = match q.cursor.as_deref() {
        None => 0,
        Some(c) => match decode_cursor(c) {
            Some(n) => n,
            None => {
                let envelope = ErrorEnvelope::new(
                    "INVALID_CURSOR",
                    "cursor token is not a recognized format",
                    rid,
                );
                return (StatusCode::UNPROCESSABLE_ENTITY, Json(envelope)).into_response();
            }
        },
    };

    let path = log_path(&root);
    if !path.exists() {
        return (
            StatusCode::OK,
            Json(StateLogPageDto {
                items: Vec::new(),
                next_cursor: None,
            }),
        )
            .into_response();
    }

    let mut file = match fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => {
            return internal(
                rid,
                "STATE_LOG_READ_FAILED",
                "could not open state update log",
                e.to_string(),
            );
        }
    };
    let total = match file.metadata().map(|m| m.len()) {
        Ok(n) => n,
        Err(e) => {
            return internal(
                rid,
                "STATE_LOG_READ_FAILED",
                "could not stat state update log",
                e.to_string(),
            );
        }
    };

    if start > total {
        let envelope = ErrorEnvelope::new("INVALID_CURSOR", "cursor points past end of log", rid);
        return (StatusCode::UNPROCESSABLE_ENTITY, Json(envelope)).into_response();
    }

    if let Err(e) = file.seek(SeekFrom::Start(start)) {
        return internal(
            rid,
            "STATE_LOG_READ_FAILED",
            "could not seek state update log",
            e.to_string(),
        );
    }

    let mut reader = BufReader::new(file);
    let mut consumed: u64 = start;
    let mut items: Vec<StateUpdateDto> = Vec::with_capacity(limit);
    let mut buf = String::new();
    while items.len() < limit {
        buf.clear();
        let n = match reader.read_line(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                return internal(
                    rid,
                    "STATE_LOG_READ_FAILED",
                    "could not read state update log",
                    e.to_string(),
                );
            }
        };
        consumed += n as u64;
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        items.push(parse_record(&v));
    }

    let next_cursor = if consumed >= total {
        None
    } else {
        Some(encode_cursor(consumed))
    };

    (StatusCode::OK, Json(StateLogPageDto { items, next_cursor })).into_response()
}

fn parse_record(v: &Value) -> StateUpdateDto {
    let slice_id = v
        .get("slice_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let event = v
        .get("event")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let at = v
        .get("at")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let schema_version = v
        .get("schema_version")
        .and_then(Value::as_u64)
        .map(|n| n as u32);
    let origin = v
        .get("origin")
        .and_then(|o| serde_json::from_value::<Origin>(o.clone()).ok())
        .as_ref()
        .map(origin_to_dto);
    let body = v.get("body").cloned();
    StateUpdateDto {
        slice_id,
        event,
        at,
        origin,
        schema_version,
        body,
    }
}

#[derive(OpenApi)]
#[openapi(
    paths(get_status, get_slices, get_state_log),
    components(schemas(
        StatusDto,
        SliceCountsDto,
        SliceQueueDto,
        SliceDto,
        StateLogPageDto,
        StateUpdateDto,
        OriginDto,
    ))
)]
pub struct WorkflowApi;
