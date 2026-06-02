//! L4-Session-002 — Question Envelope + Answer round-trip over WS.
//!
//! Acceptance:   cargo test -p mutagen-service --test session_chat
//! ISC-009:      question_envelope_always_carries_schema_version
//! INV-S4:       mismatched_answer_shape_returns_validation_error
//! FR-13:        user_reply_persists_to_elicitation_jsonl
//! FR-14/ISC-012: elicitation_jsonl_survives_socket_drop_for_reconnect
//!
//! No tokio-tungstenite in the dep tree (would widen Cargo scope), so we
//! hand-roll the WS frames. The protocol surface we exercise is tiny: a
//! single masked client text frame in, a single unmasked server text frame
//! out, repeat. RFC 6455 §5.2 is more than we need; the slice doesn't ship
//! fragmentation, control frames beyond ping, or extensions.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::Router;
use mutagen_service::observability;
use mutagen_service::routes::session::{ProjectRootResolver, SessionState, session_router};
use mutagen_service::session::ActiveSessionRegistry;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

struct FixedRoot(PathBuf);
impl ProjectRootResolver for FixedRoot {
    fn resolve(&self, _project_id: &str) -> Option<PathBuf> {
        Some(self.0.clone())
    }
}

fn fresh_state(root: Option<PathBuf>) -> SessionState {
    let s = SessionState::new(ActiveSessionRegistry::new(), "secret:test:fixture");
    match root {
        Some(r) => s.with_root_resolver(Arc::new(FixedRoot(r))),
        None => s,
    }
}

async fn boot(
    router: Router,
) -> (
    std::net::SocketAddr,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (tx, rx) = oneshot::channel::<()>();
    let h = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .expect("serve");
    });
    (addr, tx, h)
}

async fn ws_handshake(socket: &mut TcpStream, project_id: &str, host: &str) {
    let req = format!(
        "GET /projects/{project_id}/session HTTP/1.1\r\n\
         Host: {host}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\
         \r\n"
    );
    socket
        .write_all(req.as_bytes())
        .await
        .expect("handshake write");

    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 256];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let n = tokio::time::timeout_at(deadline, socket.read(&mut chunk))
            .await
            .expect("handshake timeout")
            .expect("handshake read");
        if n == 0 {
            panic!("EOF before WS handshake completed");
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf);
    assert!(
        head.starts_with("HTTP/1.1 101"),
        "expected 101 upgrade, got:\n{head}"
    );
}

async fn send_text_frame(socket: &mut TcpStream, payload: &str) {
    let bytes = payload.as_bytes();
    let mut frame = vec![0x81u8]; // FIN + text opcode
    let len = bytes.len();
    if len < 126 {
        frame.push(0x80 | len as u8);
    } else if len < 65536 {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(len as u64).to_be_bytes());
    }
    let mask = [0xA5u8, 0x3C, 0xF1, 0x09];
    frame.extend_from_slice(&mask);
    for (i, b) in bytes.iter().enumerate() {
        frame.push(b ^ mask[i % 4]);
    }
    socket.write_all(&frame).await.expect("frame write");
}

async fn read_exact(socket: &mut TcpStream, out: &mut [u8]) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut filled = 0;
    while filled < out.len() {
        let n = tokio::time::timeout_at(deadline, socket.read(&mut out[filled..]))
            .await
            .expect("frame read timeout")
            .expect("frame read");
        if n == 0 {
            panic!("EOF after {filled}/{} bytes of WS frame", out.len());
        }
        filled += n;
    }
}

async fn read_text_frame(socket: &mut TcpStream) -> String {
    loop {
        let mut header = [0u8; 2];
        read_exact(socket, &mut header).await;
        let opcode = header[0] & 0x0F;
        let masked = (header[1] & 0x80) != 0;
        assert!(!masked, "RFC 6455: server MUST NOT mask");
        let mut len = (header[1] & 0x7F) as usize;
        if len == 126 {
            let mut ext = [0u8; 2];
            read_exact(socket, &mut ext).await;
            len = u16::from_be_bytes(ext) as usize;
        } else if len == 127 {
            let mut ext = [0u8; 8];
            read_exact(socket, &mut ext).await;
            len = u64::from_be_bytes(ext) as usize;
        }
        let mut payload = vec![0u8; len];
        read_exact(socket, &mut payload).await;
        match opcode {
            0x1 => return String::from_utf8(payload).expect("utf-8 text frame"),
            0x9 => continue, // ping — server-sent pings are uncommon, but tolerate
            0x8 => panic!("server closed mid-test"),
            other => panic!("unexpected opcode {other:#x}"),
        }
    }
}

static SUFFIX: AtomicU64 = AtomicU64::new(0);

fn tempdir() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "mutagen-chat-{}-{}",
        std::process::id(),
        SUFFIX.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).expect("mkdir tempdir");
    p
}

/// Acceptance — FR-10, FR-11, FR-12. issue_question → question.issued, then
/// submit_answer → question.answered. Validates the round-trip the chat
/// protocol promises GUI clients.
#[tokio::test]
async fn round_trip_issue_question_then_submit_answer() {
    let state = fresh_state(None);
    let (addr, tx, h) = boot(observability::wrap(session_router(state))).await;
    let host = format!("{addr}");
    let mut sock = TcpStream::connect(addr).await.expect("connect");
    ws_handshake(&mut sock, "proj-roundtrip", &host).await;

    send_text_frame(
        &mut sock,
        &json!({
            "op": "issue_question",
            "prompt": "what's your name?",
            "kind": "free_text",
        })
        .to_string(),
    )
    .await;

    let issued: Value = serde_json::from_str(&read_text_frame(&mut sock).await).expect("json");
    assert_eq!(issued["event"], "question_issued");
    let envelope = &issued["envelope"];
    assert_eq!(envelope["kind"], "free_text");
    let qid = envelope["question_id"]
        .as_str()
        .expect("question_id")
        .to_owned();

    send_text_frame(
        &mut sock,
        &json!({
            "op": "submit_answer",
            "question_id": qid,
            "answer": { "kind": "free_text", "text": "april" },
        })
        .to_string(),
    )
    .await;

    let answered: Value = serde_json::from_str(&read_text_frame(&mut sock).await).expect("json");
    assert_eq!(answered["event"], "question_answered");
    assert_eq!(answered["question_id"], qid);
    assert_eq!(answered["answer_kind"], "free_text");

    drop(sock);
    tx.send(()).ok();
    h.await.ok();
}

/// ISC-009 — every Question Envelope emitted across the wire carries
/// `schema_version`. Probes the four payload-bearing kinds plus the two
/// payload-less kinds so a future Kind addition can't sneak past with a
/// missing version stamp.
#[tokio::test]
async fn question_envelope_always_carries_schema_version() {
    let state = fresh_state(None);
    let (addr, tx, h) = boot(observability::wrap(session_router(state))).await;
    let host = format!("{addr}");
    let mut sock = TcpStream::connect(addr).await.expect("connect");
    ws_handshake(&mut sock, "proj-versioned", &host).await;

    let cases: [(&str, Value); 5] = [
        ("free_text", json!(null)),
        ("boolean", json!(null)),
        ("multi_choice", json!({ "options": ["yes", "no", "maybe"] })),
        ("multi_select", json!({ "options": ["a", "b"] })),
        ("file_upload", json!({ "accept": ["image/*"] })),
    ];

    for (kind, payload) in cases {
        let mut frame = json!({ "op": "issue_question", "prompt": "q", "kind": kind });
        if !payload.is_null() {
            frame["payload"] = payload;
        }
        send_text_frame(&mut sock, &frame.to_string()).await;
        let v: Value = serde_json::from_str(&read_text_frame(&mut sock).await).expect("json");
        assert_eq!(v["event"], "question_issued", "kind {kind}");
        let sv = v["envelope"]["schema_version"]
            .as_str()
            .unwrap_or_else(|| panic!("schema_version missing for kind {kind}: {v}"));
        assert!(!sv.is_empty(), "schema_version empty for kind {kind}");
        assert_eq!(v["envelope"]["kind"], kind);
    }

    drop(sock);
    tx.send(()).ok();
    h.await.ok();
}

/// INV-S4 — answer shape that doesn't match the outstanding question's Kind
/// MUST yield `error.validation`, NOT a state transition. We probe with a
/// Boolean answer to a FreeText question, then re-submit a correct FreeText
/// answer to prove the question stayed outstanding (no state moved).
#[tokio::test]
async fn mismatched_answer_shape_returns_validation_error() {
    let state = fresh_state(None);
    let (addr, tx, h) = boot(observability::wrap(session_router(state))).await;
    let host = format!("{addr}");
    let mut sock = TcpStream::connect(addr).await.expect("connect");
    ws_handshake(&mut sock, "proj-mismatch", &host).await;

    send_text_frame(
        &mut sock,
        &json!({ "op": "issue_question", "prompt": "name?", "kind": "free_text" }).to_string(),
    )
    .await;
    let issued: Value = serde_json::from_str(&read_text_frame(&mut sock).await).expect("json");
    let qid = issued["envelope"]["question_id"]
        .as_str()
        .expect("question_id")
        .to_owned();

    // Wrong shape on purpose.
    send_text_frame(
        &mut sock,
        &json!({
            "op": "submit_answer",
            "question_id": qid,
            "answer": { "kind": "boolean", "value": true },
        })
        .to_string(),
    )
    .await;
    let err: Value = serde_json::from_str(&read_text_frame(&mut sock).await).expect("json");
    assert_eq!(err["event"], "error_validation");
    assert_eq!(err["code"], "ANSWER_SHAPE_MISMATCH");
    assert_eq!(err["question_id"], qid);

    // The question must still be outstanding — a correct answer now resolves it.
    send_text_frame(
        &mut sock,
        &json!({
            "op": "submit_answer",
            "question_id": qid,
            "answer": { "kind": "free_text", "text": "april" },
        })
        .to_string(),
    )
    .await;
    let answered: Value = serde_json::from_str(&read_text_frame(&mut sock).await).expect("json");
    assert_eq!(
        answered["event"], "question_answered",
        "question must remain outstanding after a validation error"
    );

    drop(sock);
    tx.send(()).ok();
    h.await.ok();
}

/// FR-13 — user replies received over WS MUST land in
/// `<project_root>/.mutagen/state/elicitation.jsonl` using the same format
/// April writes from the CLI plugin path.
#[tokio::test]
async fn user_reply_persists_to_elicitation_jsonl() {
    let root = tempdir();
    let state = fresh_state(Some(root.clone()));
    let (addr, tx, h) = boot(observability::wrap(session_router(state))).await;
    let host = format!("{addr}");
    let mut sock = TcpStream::connect(addr).await.expect("connect");
    ws_handshake(&mut sock, "proj-jsonl", &host).await;

    send_text_frame(
        &mut sock,
        &json!({
            "op": "issue_question",
            "prompt": "which colour?",
            "kind": "multi_choice",
            "payload": { "options": ["red", "blue"] },
        })
        .to_string(),
    )
    .await;
    let issued: Value = serde_json::from_str(&read_text_frame(&mut sock).await).expect("json");
    let qid = issued["envelope"]["question_id"]
        .as_str()
        .expect("question_id")
        .to_owned();

    send_text_frame(
        &mut sock,
        &json!({
            "op": "submit_answer",
            "question_id": qid,
            "answer": { "kind": "multi_choice", "choice": "red" },
        })
        .to_string(),
    )
    .await;
    let answered: Value = serde_json::from_str(&read_text_frame(&mut sock).await).expect("json");
    assert_eq!(answered["event"], "question_answered");

    // Give the spawned WS task a tick to flush before we read.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let log_path = root
        .join(".mutagen")
        .join("state")
        .join("elicitation.jsonl");
    let raw = std::fs::read_to_string(&log_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", log_path.display()));
    let line = raw.trim();
    assert!(
        !line.is_empty(),
        "elicitation.jsonl must have at least one record"
    );
    let record: Value = serde_json::from_str(line).expect("jsonl record");
    assert_eq!(record["origin"], "service");
    assert_eq!(record["answers_recorded"][0]["kind"], "multi_choice");
    assert_eq!(record["answers_recorded"][0]["a"], "red");
    assert_eq!(record["answers_recorded"][0]["question_id"], qid);

    drop(sock);
    tx.send(()).ok();
    h.await.ok();
}

/// FR-14 / ISC-012 — Sessions are not durable, but the elicitation
/// checkpoint is. Drop a socket mid-conversation; the file MUST survive
/// untouched so a reconnecting client (and April reading the file) can
/// resume from the last persisted reply.
#[tokio::test]
async fn elicitation_jsonl_survives_socket_drop_for_reconnect() {
    let root = tempdir();
    let state = fresh_state(Some(root.clone()));
    let registry = Arc::clone(&state.registry);
    let (addr, tx, h) = boot(observability::wrap(session_router(state))).await;
    let host = format!("{addr}");

    {
        let mut sock = TcpStream::connect(addr).await.expect("connect a");
        ws_handshake(&mut sock, "proj-reconnect", &host).await;
        send_text_frame(
            &mut sock,
            &json!({ "op": "issue_question", "prompt": "ok?", "kind": "boolean" }).to_string(),
        )
        .await;
        let issued: Value = serde_json::from_str(&read_text_frame(&mut sock).await).expect("json");
        let qid = issued["envelope"]["question_id"]
            .as_str()
            .expect("question_id")
            .to_owned();
        send_text_frame(
            &mut sock,
            &json!({
                "op": "submit_answer",
                "question_id": qid,
                "answer": { "kind": "boolean", "value": true },
            })
            .to_string(),
        )
        .await;
        let _ = read_text_frame(&mut sock).await;
        // sock drops at end of block — simulates the network blink.
    }

    // Let the server flush the close + release the registry seat.
    for _ in 0..20 {
        if registry.is_active("proj-reconnect").is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        registry.is_active("proj-reconnect").is_none(),
        "registry must release seat once the dropped socket is observed"
    );

    let log_path = root
        .join(".mutagen")
        .join("state")
        .join("elicitation.jsonl");
    let pre = std::fs::read_to_string(&log_path).expect("read elicitation.jsonl pre-reconnect");
    assert!(!pre.trim().is_empty(), "FR-13 record must be on disk");

    // Reconnect on the same project. New Session, same durable checkpoint.
    let mut sock = TcpStream::connect(addr).await.expect("connect b");
    ws_handshake(&mut sock, "proj-reconnect", &host).await;
    drop(sock);

    let post = std::fs::read_to_string(&log_path).expect("read elicitation.jsonl post-reconnect");
    assert_eq!(
        pre, post,
        "reconnect must not mutate the pre-existing elicitation log"
    );

    tx.send(()).ok();
    h.await.ok();
}
