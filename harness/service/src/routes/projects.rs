//! `POST /projects`, `GET /projects`, `GET /projects/{project_id}`,
//! `DELETE /projects/{project_id}` — FR-5, FR-6, ISC-008/010/011, DSD-300/310-312/621/624/627.
//!
//! Handler signatures take and return only DTOs from `crate::dto::*`. The
//! domain `ProjectRegistry` lives behind `State<ProjectsState>`; we translate
//! at the seam (ISC-010) so a registry refactor can't reshape the wire.

use std::sync::{Arc, Mutex};

use axum::{
    Extension, Json, Router,
    extract::{Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use mutagen_core::project::registry::{ArchiveError, LookupError, ProjectRegistry, RegisterError};
use mutagen_core::project::root::ProjectRootError;
use serde_json::{Value, json};
use utoipa::OpenApi;

use crate::dto::{ErrorEnvelope, ProjectDto, ProjectListDto, RegisterProjectRequest};
use crate::observability::RequestId;

/// Long-lived projects state. The Mutex is std::sync — handlers do brief
/// in-memory + file I/O work and release; no `.await` is held across it.
#[derive(Clone)]
pub struct ProjectsState {
    pub registry: Arc<Mutex<ProjectRegistry>>,
}

impl ProjectsState {
    pub fn new(registry: ProjectRegistry) -> Self {
        Self {
            registry: Arc::new(Mutex::new(registry)),
        }
    }
}

pub fn router(state: ProjectsState) -> Router {
    Router::new()
        .route("/projects", post(register_project).get(list_projects))
        .route(
            "/projects/:project_id",
            get(get_project).delete(archive_project),
        )
        .with_state(state)
}

#[utoipa::path(
    post,
    path = "/projects",
    tag = "projects",
    request_body = RegisterProjectRequest,
    responses(
        (status = 201, description = "Project registered", body = ProjectDto),
        (status = 409, description = "Root already registered", body = ErrorEnvelope),
        (status = 422, description = "Validation failure", body = ErrorEnvelope),
    ),
)]
pub async fn register_project(
    State(state): State<ProjectsState>,
    Extension(rid): Extension<RequestId>,
    req: Request,
) -> Response {
    let rid = rid.0;

    let body = match read_json(req).await {
        Ok(v) => v,
        Err(e) => return validation_error(rid, vec![e]),
    };

    let Some(obj) = body.as_object() else {
        return validation_error(
            rid,
            vec![field_error(
                "(root)",
                "object",
                "request body must be a JSON object",
            )],
        );
    };

    let mut missing: Vec<Value> = Vec::new();
    let root = pick_string(obj, "root", &mut missing);
    let name = pick_string(obj, "name", &mut missing);
    if !missing.is_empty() {
        return validation_error(rid, missing);
    }
    // pick_string records a missing-field error for every absent or
    // non-string field, and we returned above when any were recorded, so both
    // are Some here. Bind with let-else rather than unwrap to satisfy the
    // service crate's `-D clippy::unwrap_used` gate; the else arm is
    // unreachable but re-validates defensively instead of panicking.
    let (Some(root), Some(name)) = (root, name) else {
        return validation_error(
            rid,
            vec![field_error(
                "(root)",
                "required",
                "root and name are required",
            )],
        );
    };

    if name.trim().is_empty() {
        return validation_error(
            rid,
            vec![field_error("name", "non_empty", "name must not be empty")],
        );
    }

    let mut guard = match state.registry.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };

    match guard.register(name, &root) {
        Ok(entry) => (StatusCode::CREATED, Json(ProjectDto::from_entry(&entry))).into_response(),
        Err(RegisterError::Root(ProjectRootError::Relative)) => validation_error(
            rid,
            vec![field_error(
                "root",
                "absolute_path",
                "root must be an absolute path",
            )],
        ),
        Err(RegisterError::Root(ProjectRootError::Canonicalize(e))) => validation_error(
            rid,
            vec![field_error(
                "root",
                "canonicalize",
                &format!("could not canonicalize root: {e}"),
            )],
        ),
        Err(RegisterError::DuplicateRoot { existing_id }) => error_response(
            StatusCode::CONFLICT,
            "DUPLICATE_ROOT",
            "root already registered",
            rid,
            Some(json!({ "existing_project_id": existing_id })),
        ),
        Err(RegisterError::Persist(e)) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "REGISTRY_PERSIST_FAILED",
            "could not persist registry",
            rid,
            Some(json!({ "reason": e.to_string() })),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/projects",
    tag = "projects",
    responses(
        (status = 200, description = "Registered projects (possibly empty)", body = ProjectListDto),
    ),
)]
pub async fn list_projects(State(state): State<ProjectsState>) -> Json<ProjectListDto> {
    let guard = match state.registry.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let items = guard.list().iter().map(ProjectDto::from_entry).collect();
    Json(ProjectListDto { items })
}

#[utoipa::path(
    get,
    path = "/projects/{project_id}",
    tag = "projects",
    params(("project_id" = String, Path, description = "Project UUIDv7")),
    responses(
        (status = 200, description = "Project detail", body = ProjectDto),
        (status = 404, description = "Unknown project_id", body = ErrorEnvelope),
    ),
)]
pub async fn get_project(
    State(state): State<ProjectsState>,
    Path(project_id): Path<String>,
    Extension(rid): Extension<RequestId>,
) -> Response {
    let rid = rid.0;
    let guard = match state.registry.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    match guard.lookup(&project_id) {
        Ok(entry) => (StatusCode::OK, Json(ProjectDto::from_entry(entry))).into_response(),
        Err(LookupError::NotFound) => not_found(rid),
    }
}

#[utoipa::path(
    delete,
    path = "/projects/{project_id}",
    tag = "projects",
    params(("project_id" = String, Path, description = "Project UUIDv7")),
    responses(
        (status = 204, description = "Project archived (workspace files left intact)"),
        (status = 404, description = "Unknown project_id", body = ErrorEnvelope),
    ),
)]
pub async fn archive_project(
    State(state): State<ProjectsState>,
    Path(project_id): Path<String>,
    Extension(rid): Extension<RequestId>,
) -> Response {
    let rid = rid.0;
    let mut guard = match state.registry.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    match guard.archive(&project_id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(ArchiveError::NotFound) => not_found(rid),
        Err(ArchiveError::Persist(e)) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "REGISTRY_PERSIST_FAILED",
            "could not persist registry",
            rid,
            Some(json!({ "reason": e.to_string() })),
        ),
    }
}

async fn read_json(req: Request) -> Result<Value, Value> {
    use axum::body::to_bytes;
    let (_parts, body) = req.into_parts();
    let bytes = to_bytes(body, 64 * 1024)
        .await
        .map_err(|e| field_error("(body)", "readable", &format!("could not read body: {e}")))?;
    if bytes.is_empty() {
        return Err(field_error("(body)", "json", "request body must be JSON"));
    }
    serde_json::from_slice::<Value>(&bytes)
        .map_err(|e| field_error("(body)", "json", &format!("invalid JSON: {e}")))
}

fn pick_string(
    obj: &serde_json::Map<String, Value>,
    key: &str,
    missing: &mut Vec<Value>,
) -> Option<String> {
    match obj.get(key) {
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            missing.push(field_error(key, "string", "field must be a string"));
            None
        }
        None => {
            missing.push(field_error(key, "required", "field is required"));
            None
        }
    }
}

fn field_error(field: &str, rule: &str, message: &str) -> Value {
    json!({ "field": field, "rule": rule, "message": message })
}

fn validation_error(rid: String, items: Vec<Value>) -> Response {
    let envelope = ErrorEnvelope::new("VALIDATION_FAILED", "request body failed validation", rid)
        .with_details(Value::Array(items));
    (StatusCode::UNPROCESSABLE_ENTITY, Json(envelope)).into_response()
}

fn error_response(
    status: StatusCode,
    code: &str,
    message: &str,
    rid: String,
    details: Option<Value>,
) -> Response {
    let mut envelope = ErrorEnvelope::new(code, message, rid);
    if let Some(d) = details {
        envelope = envelope.with_details(d);
    }
    (status, Json(envelope)).into_response()
}

fn not_found(rid: String) -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "PROJECT_NOT_FOUND",
        "no project with that id",
        rid,
        None,
    )
}

#[derive(OpenApi)]
#[openapi(
    paths(register_project, list_projects, get_project, archive_project),
    components(schemas(ProjectDto, ProjectListDto, RegisterProjectRequest))
)]
pub struct ProjectsApi;
