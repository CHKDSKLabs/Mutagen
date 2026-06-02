use anyhow::{Context, Result};
use serde_json::Value;
use utoipa::OpenApi;

use crate::dto::{
    ClientMessage, CommandAcceptedDto, ConfirmEscalateDto, EmptyCommandBody, ErrorEnvelope,
    EscalateRequest, HealthDto, OriginDto, ProjectDto, ProjectListDto, QuestionEnvelopeDto,
    RegisterProjectRequest, ServerMessage, SliceCountsDto, SliceDto, SliceQueueDto,
    StateLogPageDto, StateUpdateDto, StatusDto, VersionDto,
};

/// Banner spliced into the rendered spec so a hand-edit is loud at review.
/// ISC-013 detection vector #2: the marker is regenerated on every xtask run;
/// a contributor who edits docs/openapi.json by hand and forgets to keep this
/// line will trip the drift test, and a contributor who keeps the line but
/// hand-edits anything else still trips the byte-equality check.
pub const GENERATED_MARKER: &str =
    "GENERATED — DO NOT EDIT. Run `cargo run -p xtask -- openapi` to regenerate.";

#[derive(OpenApi)]
#[openapi(
    info(
        title = "mutagen-service",
        version = "0.3.3",
        description = "Mutagen workflow service. Spec is generated from handler annotations via utoipa per ADR-0002.",
    ),
    paths(
        crate::routes::version::get_version,
        crate::routes::projects::register_project,
        crate::routes::projects::list_projects,
        crate::routes::projects::get_project,
        crate::routes::projects::archive_project,
        crate::routes::session::open_session,
        crate::routes::workflow_read::get_status,
        crate::routes::workflow_read::get_slices,
        crate::routes::workflow_read::get_state_log,
        crate::routes::workflow_write::dispatch_next,
        crate::routes::workflow_write::accept,
        crate::routes::workflow_write::escalate,
        crate::routes::workflow_write::finalize,
        crate::routes::workflow_write::resume,
        crate::routes::workflow_write::confirm_escalate,
    ),
    components(schemas(
        VersionDto,
        HealthDto,
        ErrorEnvelope,
        ProjectDto,
        ProjectListDto,
        RegisterProjectRequest,
        StatusDto,
        SliceCountsDto,
        SliceQueueDto,
        SliceDto,
        StateLogPageDto,
        StateUpdateDto,
        OriginDto,
        CommandAcceptedDto,
        EmptyCommandBody,
        EscalateRequest,
        ConfirmEscalateDto,
        // L4-Session-002 — chat protocol DTOs. The route is WS so utoipa
        // can't infer body shape from handler returns; ISC-009 / ISC-013
        // need these reachable from the spec for client codegen.
        QuestionEnvelopeDto,
        ClientMessage,
        ServerMessage,
    )),
)]
pub struct ApiDoc;

/// The fully decorated spec as a `serde_json::Value`. Splices the
/// `$comment` banner at the document root so the marker survives any
/// reformat that preserves JSON semantics.
pub fn spec_value() -> Result<Value> {
    let mut v = serde_json::to_value(ApiDoc::openapi()).context("serializing ApiDoc")?;
    if let Value::Object(map) = &mut v {
        map.insert(
            "$comment".to_string(),
            Value::String(GENERATED_MARKER.to_string()),
        );
    }
    Ok(v)
}

/// Pretty-printed JSON bytes with a trailing newline. Matches what xtask writes
/// to disk so the drift test can do a byte-equality compare.
pub fn spec_json() -> Result<Vec<u8>> {
    render(&spec_value()?)
}

pub fn render(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value).context("rendering spec to JSON")?;
    bytes.push(b'\n');
    Ok(bytes)
}
