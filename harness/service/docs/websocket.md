# WebSocket protocol — `/projects/{project_id}/session`

This document is the externalDocs target for the WebSocket route the OpenAPI
spec cannot fully express inline (utoipa renders HTTP-style request/response
shapes; the chat-protocol frame shape sits on top of an upgraded socket).

## Handshake

| Aspect                | Value                                                   |
|-----------------------|---------------------------------------------------------|
| Method                | `GET`                                                   |
| Path                  | `/projects/{project_id}/session`                        |
| Query parameter       | `schema_version` — chat protocol version, default `1`   |
| Authentication        | `Authorization: Bearer <shared-secret>` (NFR-2 / FR-7). Auth runs *before* the protocol switch; a 401 is a plain HTTP response, never an open WebSocket. |
| Upgrade headers       | Standard RFC 6455: `Upgrade: websocket`, `Connection: Upgrade`, `Sec-WebSocket-Key`, `Sec-WebSocket-Version: 13`. |
| Success status        | `101 Switching Protocols`                               |
| Conflict status       | `409 Conflict` — `ErrorEnvelope { code: SESSION_CONFLICT, details: { active_session_id: "<uuidv7>" } }` per ISC-015. |
| Bad version status    | `400 Bad Request` — `ErrorEnvelope { code: UNSUPPORTED_SCHEMA_VERSION }`. |

## Versioning

Per DSD-302 the route path is fixed at `/projects/{project_id}/session`.
Incompatible wire changes bump the `schema_version` query parameter rather
than introduce a `/v2/...` shadow route. Today the server only accepts
`schema_version=1` (matches `mutagen_core::session::CHAT_PROTOCOL_VERSION`).

## Per-session invariants

- INV-S1 / ISC-015: at most one active Session per `project_id`. A second
  upgrade attempt against the same `project_id` while a Session is active
  is rejected with 409 + `active_session_id`.
- INV-S2: Session state transitions are monotonic; the aggregate never
  walks backwards from `closed`.
- INV-S5 / POL-P2: closing the Session releases the project-scoped active
  slot, freeing the project for the next opener.
- ISC-012: Sessions are *not* durable. A service restart drops every open
  Session; clients must reconnect to begin fresh. The durable elicitation
  log under `.mutagen/state/elicitation.jsonl` is the source of truth.

## Frame schema (v1)

Frame envelope work belongs to a later Session-layer slice. As of
L4-Session-001 the upgrade succeeds, the registry seat is acquired, the
session-lifecycle aggregate is moved through `opened → authenticated →
active`, and the socket idles until the client sends a Close frame (at
which point the seat is released). The Question Envelope / Answer wire
shapes land alongside April's elicitation streaming work.

Until that lands, clients SHOULD:

- send `Close` to terminate cleanly;
- send `Ping` for liveness — the server echoes `Pong` with the same payload;
- avoid sending data frames — they are silently ignored at v1 (no defined
  semantics yet).

## Observability fields (DSD-634)

Every log line emitted inside the `session` tracing span carries:

- `session_id` — server-minted UUIDv7
- `project_id` — the path-bound project
- `principal_id` — the authenticated principal label (today
  `secret:<secret_id>`; a JWT-derived id when ADR-0004 lands)
