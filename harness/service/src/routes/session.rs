//! `GET /projects/{project_id}/session` — WebSocket upgrade route per
//! DSD-302, FR-2, ISC-015.
//!
//! The auth middleware (L3-Auth-002) runs *before* this handler is invoked,
//! so an unauthenticated upgrade attempt returns 401 from the layer above —
//! we never get a chance to switch protocols (NFR-2 satisfied at the layer
//! boundary, detection test `ws_handshake_rejects_without_auth`).
//!
//! At handler entry we attempt to seat the project's active-session slot.
//! Conflict → 409 + `ErrorEnvelope { code: SESSION_CONFLICT, details:
//! { active_session_id } }` per ISC-015. Acquired → the seat-guard moves
//! into the `on_upgrade` closure and lives for the duration of the socket
//! loop; Drop releases the seat (INV-S5 / POL-P2).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use mutagen_core::session::{
    CHAT_PROTOCOL_VERSION, QuestionEnvelope, QuestionKind, Session, append_user_reply,
    validate_answer,
};
use serde::Deserialize;
use serde_json::json;
use tracing::Instrument;
use tracing::instrument::WithSubscriber;

use crate::dto::ErrorEnvelope;
use crate::dto::session::{ClientMessage, ServerMessage};
use crate::observability::RequestId;
use crate::session::registry::{AcquireOutcome, ActiveSessionRegistry, SessionLock};

// The slice contract puts the broadcaster at harness/service/src/session/broadcast.rs
// but `session/mod.rs` is outside this slice's write scope. Pulling the file in
// here via #[path] keeps it on disk at the canonical location while letting
// routes/session own the registration. Public so tests + downstream wiring can
// reach it as `crate::routes::session::broadcast::ProjectBroadcaster`.
#[path = "../session/broadcast.rs"]
pub mod broadcast;

use broadcast::{BroadcastEvent, ProjectBroadcaster};
use tokio::sync::broadcast::error::RecvError as BroadcastRecvError;

/// Hook the route uses to resolve `<project_root>` for FR-13 persistence.
/// Today's main() wiring leaves it `None` (per-command project plumb-through
/// lands in L4-Session-003); tests inject a tempdir-backed resolver.
pub trait ProjectRootResolver: Send + Sync + 'static {
    fn resolve(&self, project_id: &str) -> Option<PathBuf>;
}

impl<F> ProjectRootResolver for F
where
    F: Fn(&str) -> Option<PathBuf> + Send + Sync + 'static,
{
    fn resolve(&self, project_id: &str) -> Option<PathBuf> {
        (self)(project_id)
    }
}

#[derive(Clone)]
pub struct SessionState {
    pub registry: Arc<ActiveSessionRegistry>,
    /// Label stamped onto the Session aggregate's `principal_id`. Today this
    /// is the auth secret's stable id (`secret:{secret_id}`); when the auth
    /// chain plumbs a real Principal through request extensions, this fallback
    /// goes away. Follow-up — see State Update.
    pub principal_label: Arc<str>,
    /// Optional project_id → project_root resolver. When `Some`, user replies
    /// are persisted to `<root>/.mutagen/state/elicitation.jsonl` per FR-13.
    /// When `None`, the round-trip still works in-memory but no JSONL append
    /// happens (tests opt in; production wiring lands in L4-Session-003).
    pub root_resolver: Option<Arc<dyn ProjectRootResolver>>,
    /// Per-project event fan-out. Sessions subscribe at upgrade time (POL-S4
    /// / FR-16). REST writers (workflow_write.rs, future slice) call
    /// `broadcaster.send(project_id, ..)` to push events. Default is an
    /// empty broadcaster — fine for tests that don't exercise the fan-out.
    pub broadcaster: ProjectBroadcaster,
}

impl SessionState {
    pub fn new(registry: ActiveSessionRegistry, principal_label: impl Into<Arc<str>>) -> Self {
        Self {
            registry: Arc::new(registry),
            principal_label: principal_label.into(),
            root_resolver: None,
            broadcaster: ProjectBroadcaster::new(),
        }
    }

    pub fn with_root_resolver(mut self, resolver: Arc<dyn ProjectRootResolver>) -> Self {
        self.root_resolver = Some(resolver);
        self
    }

    pub fn with_broadcaster(mut self, broadcaster: ProjectBroadcaster) -> Self {
        self.broadcaster = broadcaster;
        self
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct SessionUpgradeQuery {
    /// DSD-302: revisions of the WS protocol use a `schema_version` query
    /// parameter rather than `/v2/...`. v1 accepts the param if it equals
    /// `CHAT_PROTOCOL_VERSION`; absent means "current".
    #[serde(default)]
    pub schema_version: Option<String>,
}

/// Mount the session route. Wire under [`crate::auth::auth_wrap`] so the
/// upgrade is authenticated *before* the protocol switch happens.
pub fn session_router(state: SessionState) -> Router {
    Router::new()
        .route("/projects/:project_id/session", get(open_session))
        .with_state(state)
}

#[utoipa::path(
    get,
    path = "/projects/{project_id}/session",
    tag = "session",
    params(
        ("project_id" = String, Path, description = "Project UUIDv7."),
        (
            "schema_version" = Option<String>,
            Query,
            description = "DSD-302: chat-protocol schema version. Omit for current. \
                           See harness/service/docs/websocket.md for wire shape."
        ),
    ),
    // L4-Session-002: GET has no real body, but utoipa needs a referenced
    // schema to register the client-frame DTO in components. Downstream GUI
    // codegen pins to this shape; the route description above makes clear
    // this is informational, not a literal request body.
    request_body(content = ClientMessage, description = "Client→server WS text-frame shape (informational; HTTP GET carries no body)."),
    responses(
        (status = 101, description = "WebSocket upgrade succeeded. See harness/service/docs/websocket.md for the chat-protocol frame schema (utoipa cannot fully express WS shape inline)."),
        // Status "default": schema reference only. Pulls ServerMessage (and
        // transitively QuestionEnvelopeDto, ISC-009) into components.
        (status = "default", description = "Server→client WS text-frame shape (informational; never emitted as an HTTP response).", body = ServerMessage),
        (status = 400, description = "Unsupported schema_version.", body = ErrorEnvelope),
        (status = 401, description = "Auth middleware rejected the upgrade handshake before the protocol switch.", body = ErrorEnvelope),
        (status = 409, description = "Another session is already active on this project (ISC-015). Details carry `active_session_id`.", body = ErrorEnvelope),
    ),
)]
pub async fn open_session(
    State(state): State<SessionState>,
    Path(project_id): Path<String>,
    Query(query): Query<SessionUpgradeQuery>,
    Extension(rid): Extension<RequestId>,
    ws: WebSocketUpgrade,
) -> Response {
    let request_id = rid.0.clone();

    if let Some(v) = &query.schema_version
        && v != CHAT_PROTOCOL_VERSION
    {
        let envelope = ErrorEnvelope::new(
            "UNSUPPORTED_SCHEMA_VERSION",
            "client requested a chat schema version this server does not speak",
            request_id,
        )
        .with_details(json!({
            "requested": v,
            "supported": CHAT_PROTOCOL_VERSION,
        }));
        return (StatusCode::BAD_REQUEST, Json(envelope)).into_response();
    }

    let session = Session::open(project_id.clone(), state.principal_label.to_string());

    let lock = match state.registry.try_acquire(&project_id, &session.session_id) {
        AcquireOutcome::Acquired(lock) => lock,
        AcquireOutcome::Conflict { active_session_id } => {
            let envelope = ErrorEnvelope::new(
                "SESSION_CONFLICT",
                "another session is already active on this project",
                request_id,
            )
            .with_details(json!({ "active_session_id": active_session_id }));
            return (StatusCode::CONFLICT, Json(envelope)).into_response();
        }
    };

    let opened_session_id = session.session_id.clone();
    let opened_project_id = session.project_id.clone();
    let opened_principal_id = session.principal_id.clone();
    let project_root = state
        .root_resolver
        .as_ref()
        .and_then(|r| r.resolve(&project_id));

    // Subscribe *before* on_upgrade so any event emitted between the seat
    // acquire and the first poll of the socket task is delivered. The
    // Receiver is moved into the upgrade future; Drop on socket close is
    // the natural unsubscribe — no manual de-register call.
    let event_rx = state.broadcaster.subscribe(&project_id);

    // DSD-634: the `session` span carries session_id, project_id, principal_id
    // for every log line emitted inside the socket loop.
    let session_span = tracing::info_span!(
        "session",
        session_id = %session.session_id,
        project_id = %session.project_id,
        principal_id = %session.principal_id,
    );

    // session.opened + session.closed both emit from inside the upgrade
    // future. The future is instrumented with the session span (so span
    // fields decorate every line) and `.with_current_subscriber()` pins the
    // request-thread dispatcher to the future so log capture survives
    // axum's tokio::spawn of the upgrade task onto an arbitrary worker.
    //
    // The rebuild_interest_cache() call is load-bearing for parallel test
    // capture: tracing caches Interest per (callsite, dispatcher) pair, and
    // when sibling tests run in parallel without a fixture subscriber, the
    // global no-op dispatcher pins the callsite to Interest::Never. A new
    // per-test `set_default` does NOT auto-rebuild the cache (tracing-core
    // contract), so the fresh subscriber never sees register_callsite for
    // this site and silently drops the emit. Rebuilding here is cheap (one
    // upgrade per session) and keeps DSD-634 conformance honest.
    ws.on_upgrade(move |socket| {
        async move {
            tracing::callsite::rebuild_interest_cache();
            tracing::info!(
                event = "session.opened",
                session_id = %opened_session_id,
                project_id = %opened_project_id,
                principal_id = %opened_principal_id,
                "session.opened",
            );
            handle_socket(socket, lock, session, project_root, event_rx).await;
            tracing::info!(
                event = "session.closed",
                session_id = %opened_session_id,
                project_id = %opened_project_id,
                principal_id = %opened_principal_id,
                "session.closed",
            );
        }
        .instrument(session_span)
        .with_current_subscriber()
    })
}

async fn handle_socket(
    mut socket: WebSocket,
    lock: SessionLock,
    mut session: Session,
    project_root: Option<PathBuf>,
    mut event_rx: tokio::sync::broadcast::Receiver<BroadcastEvent>,
) {
    // Walk aggregate to `active` so question issuance is legal (DDD §3.3
    // command pre-conditions). Bad transitions can't happen here — the
    // aggregate was freshly opened — but the type forces us to surface them.
    let _ = session.authenticate();
    let _ = session.activate();

    let mut outstanding: HashMap<String, QuestionEnvelope> = HashMap::new();

    loop {
        tokio::select! {
            client = socket.recv() => {
                let Some(msg) = client else { break };
                match msg {
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(Message::Ping(p)) => {
                        if socket.send(Message::Pong(p)).await.is_err() { break; }
                    }
                    Ok(Message::Text(raw)) => {
                        if !handle_text(
                            &raw,
                            &mut socket,
                            &session,
                            &mut outstanding,
                            project_root.as_deref(),
                        )
                        .await
                        {
                            break;
                        }
                    }
                    Ok(_) => {}
                }
            }
            evt = event_rx.recv() => {
                match evt {
                    Ok(e) => {
                        if !forward_broadcast(&mut socket, &session, e).await { break; }
                    }
                    // Lagged means we fell behind the channel's capacity. Drop the gap
                    // and keep going — losing a stale state-update to a slow socket is
                    // better than tearing down the Session. The on-disk log is authoritative
                    // if the client needs to reconcile.
                    Err(BroadcastRecvError::Lagged(_)) => continue,
                    Err(BroadcastRecvError::Closed) => break,
                }
            }
        }
    }

    let _ = session.begin_close();
    let _ = session.close();
    drop(lock);
}

/// POL-S4 wire mapping. CommandAccepted lands as the existing
/// `ServerMessage::CommandAccepted` variant so OpenAPI codegen already
/// describes the shape (DSD-322). StateUpdated has no v1 DTO variant —
/// we emit it as raw JSON mirroring the on-disk State Update record's
/// observable fields. Returns false on socket send failure so the caller
/// can tear down the loop.
async fn forward_broadcast(socket: &mut WebSocket, session: &Session, evt: BroadcastEvent) -> bool {
    let raw = match evt {
        BroadcastEvent::CommandAccepted {
            request_id,
            command,
            at: _at,
        } => {
            match serde_json::to_string(&ServerMessage::CommandAccepted {
                session_id: session.session_id.clone(),
                request_id,
                command,
            }) {
                Ok(s) => s,
                Err(_) => return false,
            }
        }
        BroadcastEvent::StateUpdated {
            request_id,
            slice_id,
            event,
            at,
        } => {
            // Event field carries the DDD domain-event name verbatim
            // (e.g. "slice.transitioned"); GUI clients match on string.
            let body = serde_json::json!({
                "event": event,
                "session_id": session.session_id,
                "slice_id": slice_id,
                "request_id": request_id,
                "at": at,
            });
            body.to_string()
        }
    };
    socket.send(Message::Text(raw)).await.is_ok()
}

/// Returns `false` to signal the socket should close (peer write failure).
async fn handle_text(
    raw: &str,
    socket: &mut WebSocket,
    session: &Session,
    outstanding: &mut HashMap<String, QuestionEnvelope>,
    project_root: Option<&std::path::Path>,
) -> bool {
    let parsed: Result<ClientMessage, _> = serde_json::from_str(raw);
    let client_msg = match parsed {
        Ok(m) => m,
        Err(e) => {
            return send(
                socket,
                &ServerMessage::ErrorValidation {
                    session_id: session.session_id.clone(),
                    question_id: None,
                    code: "MALFORMED_CLIENT_FRAME".into(),
                    message: format!("could not parse frame: {e}"),
                },
            )
            .await;
        }
    };

    match client_msg {
        ClientMessage::IssueQuestion {
            prompt,
            kind,
            payload,
        } => {
            let qkind = match parse_kind(&kind, payload.as_ref()) {
                Some(k) => k,
                None => {
                    return send(
                        socket,
                        &ServerMessage::ErrorValidation {
                            session_id: session.session_id.clone(),
                            question_id: None,
                            code: "UNKNOWN_QUESTION_KIND".into(),
                            message: format!("kind '{kind}' is not part of v1 taxonomy"),
                        },
                    )
                    .await;
                }
            };
            let env = QuestionEnvelope::new(qkind, prompt);
            outstanding.insert(env.question_id.clone(), env.clone());
            send(
                socket,
                &ServerMessage::QuestionIssued {
                    session_id: session.session_id.clone(),
                    envelope: (&env).into(),
                },
            )
            .await
        }
        ClientMessage::SubmitAnswer {
            question_id,
            answer,
        } => {
            let env = match outstanding.get(&question_id) {
                Some(e) => e.clone(),
                None => {
                    return send(
                        socket,
                        &ServerMessage::ErrorValidation {
                            session_id: session.session_id.clone(),
                            question_id: Some(question_id),
                            code: "UNKNOWN_QUESTION_ID".into(),
                            message: "no outstanding question with that id".into(),
                        },
                    )
                    .await;
                }
            };
            if let Err(err) = validate_answer(&env, &answer) {
                // INV-S4: shape mismatch — emit validation error, do NOT
                // remove the question from `outstanding` (no state transition).
                return send(
                    socket,
                    &ServerMessage::ErrorValidation {
                        session_id: session.session_id.clone(),
                        question_id: Some(question_id),
                        code: "ANSWER_SHAPE_MISMATCH".into(),
                        message: err.to_string(),
                    },
                )
                .await;
            }
            outstanding.remove(&question_id);
            if let Some(root) = project_root
                && let Err(e) = append_user_reply(root, &env, &answer, "submit_answer via WS")
            {
                tracing::warn!(error = %e, "elicitation.jsonl append failed (FR-13)");
            }
            send(
                socket,
                &ServerMessage::QuestionAnswered {
                    session_id: session.session_id.clone(),
                    question_id,
                    answer_kind: answer.kind_name().to_owned(),
                },
            )
            .await
        }
    }
}

fn parse_kind(name: &str, payload: Option<&serde_json::Value>) -> Option<QuestionKind> {
    let opts = |key: &str| -> Vec<String> {
        payload
            .and_then(|p| p.get(key))
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    match name {
        "free_text" => Some(QuestionKind::FreeText),
        "boolean" => Some(QuestionKind::Boolean),
        "multi_choice" => Some(QuestionKind::MultiChoice {
            options: opts("options"),
        }),
        "multi_select" => Some(QuestionKind::MultiSelect {
            options: opts("options"),
        }),
        "file_upload" => Some(QuestionKind::FileUpload {
            accept: opts("accept"),
        }),
        _ => None,
    }
}

async fn send(socket: &mut WebSocket, msg: &ServerMessage) -> bool {
    let raw = match serde_json::to_string(msg) {
        Ok(s) => s,
        Err(_) => return false,
    };
    socket.send(Message::Text(raw)).await.is_ok()
}
