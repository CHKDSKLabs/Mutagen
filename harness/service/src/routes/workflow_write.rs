//! POST /projects/{id}/dispatch-next, .../slices/{slice_id}/{accept,escalate,
//! finalize,resume,confirm-escalate} — FR-1 / FR-15, DDD §3.1 Commands,
//! ISC-004/005/006/007/010/013/014, DSD-300/301/322/330/331/621/624/627.
//!
//! Each handler acquires the Project Lock for the duration of one command
//! and releases on drop (POL-S3 / ISC-005). On lock contention we return
//! 409 PROJECT_LOCKED with the holder identity. Destructive commands
//! (escalate) demand a single-use token (DSD-330) from confirm-escalate.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    Extension, Json, Router,
    extract::{Path as AxumPath, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use mutagen_core::project::lock::{self, AcquireError, LockHolder};
use mutagen_core::project::registry::LookupError;
use mutagen_core::workflow::origin::Origin;
use mutagen_core::workflow::state_update::{SCHEMA_VERSION, StateUpdate, append_record};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use utoipa::OpenApi;

use crate::dto::{
    CommandAcceptedDto, ConfirmEscalateDto, EmptyCommandBody, ErrorEnvelope, EscalateRequest,
};
use crate::observability::{RequestId, request_id::generate_uuid_v7};
use crate::routes::projects::ProjectsState;

const TOKEN_TTL: Duration = Duration::from_secs(300);

// token string -> (project_id, slice_id, minted_at). The project id rides along
// in the value so a token can't be replayed against a different project.
type EscalateTokenMap = HashMap<String, (String, String, Instant)>;

/// In-memory escalate token registry. Per-process by design — a service
/// restart invalidates outstanding tokens, which is the safe default for
/// a destructive-command gate (DSD-330).
#[derive(Clone, Default)]
pub struct EscalateTokens {
    // Key is (project_id, slice_id, minted_at). Project id lives in the
    // value so Tiger Claw's cross-project leak (a token minted against
    // project α cannot escalate project β's slice with the same id) is
    // mechanically impossible — consume() refuses to remove unless every
    // tuple element matches.
    inner: Arc<Mutex<EscalateTokenMap>>,
}

impl EscalateTokens {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, EscalateTokenMap> {
        match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    fn mint(&self, project_id: &str, slice_id: &str) -> String {
        let token = generate_uuid_v7();
        let mut guard = self.lock_inner();
        guard.retain(|_, (_, _, ts)| ts.elapsed() < TOKEN_TTL);
        guard.insert(
            token.clone(),
            (project_id.to_string(), slice_id.to_string(), Instant::now()),
        );
        token
    }

    fn consume(&self, token: &str, project_id: &str, slice_id: &str) -> bool {
        let mut guard = self.lock_inner();
        guard.retain(|_, (_, _, ts)| ts.elapsed() < TOKEN_TTL);
        let ok = matches!(
            guard.get(token),
            Some((pid, sid, _)) if pid == project_id && sid == slice_id
        );
        ok && guard.remove(token).is_some()
    }
}

#[derive(Clone)]
pub struct WorkflowWriteState {
    pub projects: ProjectsState,
    pub tokens: EscalateTokens,
}

impl WorkflowWriteState {
    pub fn new(projects: ProjectsState) -> Self {
        Self {
            projects,
            tokens: EscalateTokens::new(),
        }
    }
}

pub fn router(state: WorkflowWriteState) -> Router {
    Router::new()
        .route("/projects/:project_id/dispatch-next", post(dispatch_next))
        .route(
            "/projects/:project_id/slices/:slice_id/accept",
            post(accept),
        )
        .route(
            "/projects/:project_id/slices/:slice_id/escalate",
            post(escalate),
        )
        .route(
            "/projects/:project_id/slices/:slice_id/finalize",
            post(finalize),
        )
        .route(
            "/projects/:project_id/slices/:slice_id/resume",
            post(resume),
        )
        .route(
            "/projects/:project_id/slices/:slice_id/confirm-escalate",
            post(confirm_escalate),
        )
        .with_state(state)
}

fn resolve_root(state: &ProjectsState, project_id: &str) -> Option<PathBuf> {
    let guard = match state.registry.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    match guard.lookup(project_id) {
        Ok(entry) => Some(entry.root.clone()),
        Err(LookupError::NotFound) => None,
    }
}

fn now_iso() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn envelope(
    status: StatusCode,
    code: &str,
    message: &str,
    rid: String,
    details: Option<Value>,
) -> Response {
    let mut e = ErrorEnvelope::new(code, message, rid);
    if let Some(d) = details {
        e = e.with_details(d);
    }
    (status, Json(e)).into_response()
}

fn not_found(rid: String) -> Response {
    envelope(
        StatusCode::NOT_FOUND,
        "PROJECT_NOT_FOUND",
        "no project with that id",
        rid,
        None,
    )
}

fn locked(rid: String, holder: &str, since: &str) -> Response {
    let message = format!("lock held by {holder} · retry once the holder releases");
    envelope(
        StatusCode::CONFLICT,
        "PROJECT_LOCKED",
        &message,
        rid,
        Some(json!({ "holder": holder, "acquired_at": since })),
    )
}

fn internal(rid: String, code: &str, reason: String) -> Response {
    envelope(
        StatusCode::INTERNAL_SERVER_ERROR,
        code,
        "internal error",
        rid,
        Some(json!({ "reason": reason })),
    )
}

/// Run a Workflow Command: resolve project, acquire the lock with
/// service:<request_id> origin, append one State Update, release lock
/// on drop, return 202. `slice_id == ""` is the dispatch_next shape and
/// surfaces as `slice_id: null` in the response (slice chosen on the WS).
fn run_command(
    state: &WorkflowWriteState,
    project_id: &str,
    slice_id: &str,
    command: &str,
    event: &str,
    body: Option<Value>,
    rid: &str,
) -> Response {
    let Some(root) = resolve_root(&state.projects, project_id) else {
        return not_found(rid.to_string());
    };

    let guard = match lock::acquire(
        &root,
        LockHolder::Service {
            session_id: rid.to_string(),
        },
    ) {
        Ok(g) => g,
        Err(AcquireError::Held(rec)) => {
            return locked(rid.to_string(), &rec.holder.to_string(), &rec.acquired_at);
        }
        Err(AcquireError::HeldAnonymous) => return locked(rid.to_string(), "unknown", "unknown"),
        Err(AcquireError::Io(e)) => {
            return internal(rid.to_string(), "LOCK_IO_FAILED", e.to_string());
        }
    };

    let origin = match Origin::service(rid) {
        Ok(o) => o,
        Err(e) => return internal(rid.to_string(), "ORIGIN_INVALID", e.to_string()),
    };

    let at = now_iso();
    let record = StateUpdate {
        schema_version: SCHEMA_VERSION,
        slice_id: slice_id.to_string(),
        event: event.to_string(),
        at: at.clone(),
        origin,
        body,
    };

    if let Err(e) = append_record(&guard, &root, &record) {
        return internal(rid.to_string(), "STATE_UPDATE_WRITE_FAILED", e.to_string());
    }
    drop(guard);

    let dto = CommandAcceptedDto {
        command: command.to_string(),
        project_id: project_id.to_string(),
        slice_id: (!slice_id.is_empty()).then(|| slice_id.to_string()),
        request_id: rid.to_string(),
        accepted_at: at,
    };
    (StatusCode::ACCEPTED, Json(dto)).into_response()
}

async fn read_json_or_empty(req: Request) -> Result<Value, String> {
    use axum::body::to_bytes;
    let (_p, body) = req.into_parts();
    let bytes = to_bytes(body, 64 * 1024)
        .await
        .map_err(|e| format!("body unreadable: {e}"))?;
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice::<Value>(&bytes).map_err(|e| format!("invalid JSON: {e}"))
}

fn opt_body(v: Value) -> Option<Value> {
    if v.is_null() { None } else { Some(v) }
}

#[utoipa::path(
    post, path = "/projects/{project_id}/dispatch-next", tag = "workflow",
    params(("project_id" = String, Path, description = "Project UUIDv7")),
    responses(
        (status = 202, description = "Command accepted; completion broadcast on WS", body = CommandAcceptedDto),
        (status = 404, description = "Unknown project_id", body = ErrorEnvelope),
        (status = 409, description = "Project lock held by another writer", body = ErrorEnvelope),
    ),
)]
pub async fn dispatch_next(
    State(state): State<WorkflowWriteState>,
    AxumPath(project_id): AxumPath<String>,
    Extension(rid): Extension<RequestId>,
) -> Response {
    run_command(
        &state,
        &project_id,
        "",
        "dispatch_next",
        "cohort.dispatched",
        None,
        &rid.0,
    )
}

async fn body_command(
    state: WorkflowWriteState,
    project_id: String,
    slice_id: String,
    command: &str,
    event: &str,
    rid: String,
    req: Request,
) -> Response {
    let body = match read_json_or_empty(req).await {
        Ok(v) => opt_body(v),
        Err(e) => {
            return envelope(
                StatusCode::UNPROCESSABLE_ENTITY,
                "VALIDATION_FAILED",
                &e,
                rid,
                None,
            );
        }
    };
    run_command(&state, &project_id, &slice_id, command, event, body, &rid)
}

#[utoipa::path(
    post, path = "/projects/{project_id}/slices/{slice_id}/accept", tag = "workflow",
    params(
        ("project_id" = String, Path, description = "Project UUIDv7"),
        ("slice_id" = String, Path, description = "Slice id"),
    ),
    request_body = EmptyCommandBody,
    responses(
        (status = 202, description = "Slice accept_review recorded", body = CommandAcceptedDto),
        (status = 404, description = "Unknown project_id", body = ErrorEnvelope),
        (status = 409, description = "Project lock held by another writer", body = ErrorEnvelope),
    ),
)]
pub async fn accept(
    State(state): State<WorkflowWriteState>,
    AxumPath((project_id, slice_id)): AxumPath<(String, String)>,
    Extension(rid): Extension<RequestId>,
    req: Request,
) -> Response {
    body_command(
        state,
        project_id,
        slice_id,
        "accept_review",
        "slice.transitioned",
        rid.0,
        req,
    )
    .await
}

#[utoipa::path(
    post, path = "/projects/{project_id}/slices/{slice_id}/finalize", tag = "workflow",
    params(
        ("project_id" = String, Path, description = "Project UUIDv7"),
        ("slice_id" = String, Path, description = "Slice id"),
    ),
    request_body = EmptyCommandBody,
    responses(
        (status = 202, description = "Slice finalize recorded", body = CommandAcceptedDto),
        (status = 404, description = "Unknown project_id", body = ErrorEnvelope),
        (status = 409, description = "Project lock held by another writer", body = ErrorEnvelope),
    ),
)]
pub async fn finalize(
    State(state): State<WorkflowWriteState>,
    AxumPath((project_id, slice_id)): AxumPath<(String, String)>,
    Extension(rid): Extension<RequestId>,
    req: Request,
) -> Response {
    body_command(
        state,
        project_id,
        slice_id,
        "finalize_slice",
        "slice.transitioned",
        rid.0,
        req,
    )
    .await
}

#[utoipa::path(
    post, path = "/projects/{project_id}/slices/{slice_id}/resume", tag = "workflow",
    params(
        ("project_id" = String, Path, description = "Project UUIDv7"),
        ("slice_id" = String, Path, description = "Slice id"),
    ),
    request_body = EmptyCommandBody,
    responses(
        (status = 202, description = "Slice resume recorded", body = CommandAcceptedDto),
        (status = 404, description = "Unknown project_id", body = ErrorEnvelope),
        (status = 409, description = "Project lock held by another writer", body = ErrorEnvelope),
    ),
)]
pub async fn resume(
    State(state): State<WorkflowWriteState>,
    AxumPath((project_id, slice_id)): AxumPath<(String, String)>,
    Extension(rid): Extension<RequestId>,
    req: Request,
) -> Response {
    body_command(
        state,
        project_id,
        slice_id,
        "resume_slice",
        "slice.transitioned",
        rid.0,
        req,
    )
    .await
}

#[utoipa::path(
    post, path = "/projects/{project_id}/slices/{slice_id}/escalate", tag = "workflow",
    params(
        ("project_id" = String, Path, description = "Project UUIDv7"),
        ("slice_id" = String, Path, description = "Slice id"),
    ),
    request_body = EscalateRequest,
    responses(
        (status = 202, description = "Slice escalated; State Update written", body = CommandAcceptedDto),
        (status = 404, description = "Unknown project_id", body = ErrorEnvelope),
        (status = 409, description = "Project lock held by another writer", body = ErrorEnvelope),
        (status = 422, description = "Missing or stale confirmation token", body = ErrorEnvelope),
    ),
)]
pub async fn escalate(
    State(state): State<WorkflowWriteState>,
    AxumPath((project_id, slice_id)): AxumPath<(String, String)>,
    Extension(rid): Extension<RequestId>,
    req: Request,
) -> Response {
    let rid_s = rid.0.clone();
    let raw = match read_json_or_empty(req).await {
        Ok(v) => v,
        Err(e) => {
            return envelope(
                StatusCode::UNPROCESSABLE_ENTITY,
                "VALIDATION_FAILED",
                &e,
                rid_s,
                None,
            );
        }
    };
    let parsed: EscalateRequest = match serde_json::from_value(raw) {
        Ok(p) => p,
        Err(_) => {
            return envelope(
                StatusCode::UNPROCESSABLE_ENTITY,
                "CONFIRMATION_REQUIRED",
                "escalate requires a confirmation_token from confirm-escalate",
                rid_s,
                None,
            );
        }
    };
    if parsed.confirmation_token.trim().is_empty()
        || !state
            .tokens
            .consume(&parsed.confirmation_token, &project_id, &slice_id)
    {
        return envelope(
            StatusCode::UNPROCESSABLE_ENTITY,
            "CONFIRMATION_REQUIRED",
            "confirmation_token is missing, expired, or not for this slice",
            rid_s,
            None,
        );
    }
    let body = parsed.reason.map(|r| json!({ "reason": r }));
    run_command(
        &state,
        &project_id,
        &slice_id,
        "escalate",
        "workflow.escalated",
        body,
        &rid.0,
    )
}

#[utoipa::path(
    post, path = "/projects/{project_id}/slices/{slice_id}/confirm-escalate", tag = "workflow",
    params(
        ("project_id" = String, Path, description = "Project UUIDv7"),
        ("slice_id" = String, Path, description = "Slice id"),
    ),
    responses(
        (status = 200, description = "Single-use confirmation token for /escalate", body = ConfirmEscalateDto),
        (status = 404, description = "Unknown project_id", body = ErrorEnvelope),
    ),
)]
pub async fn confirm_escalate(
    State(state): State<WorkflowWriteState>,
    AxumPath((project_id, slice_id)): AxumPath<(String, String)>,
    Extension(rid): Extension<RequestId>,
) -> Response {
    if resolve_root(&state.projects, &project_id).is_none() {
        return not_found(rid.0);
    }
    let token = state.tokens.mint(&project_id, &slice_id);
    let expires = OffsetDateTime::now_utc() + time::Duration::seconds(TOKEN_TTL.as_secs() as i64);
    let expires_at = expires
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
    let dto = ConfirmEscalateDto {
        confirmation_token: token,
        slice_id,
        expires_at,
    };
    (StatusCode::OK, Json(dto)).into_response()
}

#[derive(OpenApi)]
#[openapi(
    paths(dispatch_next, accept, escalate, finalize, resume, confirm_escalate),
    components(schemas(
        CommandAcceptedDto,
        EmptyCommandBody,
        EscalateRequest,
        ConfirmEscalateDto
    ))
)]
pub struct WorkflowWriteApi;
