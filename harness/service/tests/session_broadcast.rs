//! L4-Session-003 — two-stage command feedback + State Update broadcast.
//!
//! Acceptance:           cargo test -p mutagen-service --test session_broadcast
//! ISC-012 detection:    session_b_sees_no_replay_after_a_closed
//! DSD-320 conformance:  command_accepted_within_100ms
//!
//! The WS framing here is intentionally minimal — same hand-rolled approach
//! the L4-Session-002 chat test uses, so we don't drag tokio-tungstenite
//! into the dep tree just to assert event order. ISC-015 means only one
//! Session ever holds a project at a time, so there's no "two sockets on
//! one project" scenario to set up — the no-replay test closes A before
//! opening B.

use std::time::{Duration, Instant};

use mutagen_service::observability;
use mutagen_service::routes::session::broadcast::{BroadcastEvent, ProjectBroadcaster};
use mutagen_service::routes::session::{SessionState, session_router};
use mutagen_service::session::ActiveSessionRegistry;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

fn state_with(broadcaster: ProjectBroadcaster) -> SessionState {
    SessionState::new(ActiveSessionRegistry::new(), "secret:test:broadcast")
        .with_broadcaster(broadcaster)
}

async fn boot(
    broadcaster: ProjectBroadcaster,
) -> (
    std::net::SocketAddr,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let router = observability::wrap(session_router(state_with(broadcaster)));
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
        "expected 101, got:\n{head}"
    );
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
            panic!("EOF after {filled}/{} bytes", out.len());
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
        assert!(!masked, "server must not mask");
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
            0x1 => return String::from_utf8(payload).expect("utf-8"),
            0x9 => continue,
            0x8 => panic!("server closed mid-test"),
            other => panic!("unexpected opcode {other:#x}"),
        }
    }
}

/// Try to read a frame with a budget; returns None on timeout (no message).
async fn try_read_text_frame(socket: &mut TcpStream, budget: Duration) -> Option<String> {
    tokio::time::timeout(budget, read_text_frame(socket))
        .await
        .ok()
}

fn cmd_accepted(rid: &str) -> BroadcastEvent {
    BroadcastEvent::CommandAccepted {
        request_id: rid.to_owned(),
        command: "dispatch_next".to_owned(),
        at: "2026-05-12T00:00:00Z".to_owned(),
    }
}

fn slice_transitioned(rid: &str, slice_id: &str) -> BroadcastEvent {
    BroadcastEvent::StateUpdated {
        request_id: rid.to_owned(),
        slice_id: Some(slice_id.to_owned()),
        event: "slice.transitioned".to_owned(),
        at: "2026-05-12T00:00:00Z".to_owned(),
    }
}

/// Acceptance — FR-16, POL-S4 / MD-10. Open Session A on project X, fire a
/// command.accepted then a slice.transitioned through the broadcaster, and
/// assert both arrive on A's socket in that order.
#[tokio::test]
async fn session_a_sees_command_accepted_then_slice_transitioned() {
    let broadcaster = ProjectBroadcaster::new();
    let (addr, tx, h) = boot(broadcaster.clone()).await;
    let host = format!("{addr}");

    let mut sock = TcpStream::connect(addr).await.expect("connect");
    ws_handshake(&mut sock, "proj-broadcast", &host).await;

    // The subscribe happens inside open_session before on_upgrade returns,
    // so a brief tick lets the upgrade task install its receiver before we
    // emit. Without it, send() could land before any subscriber exists and
    // the event would be dropped (broadcast::Sender returns 0 receivers).
    for _ in 0..40 {
        if broadcaster.send("proj-broadcast", cmd_accepted("warmup")) > 0 {
            // We just sent a real event though — drain it.
            let _ = read_text_frame(&mut sock).await;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    assert_eq!(broadcaster.send("proj-broadcast", cmd_accepted("req-1")), 1);
    let f1: Value = serde_json::from_str(&read_text_frame(&mut sock).await).expect("json");
    assert_eq!(f1["event"], "command_accepted");
    assert_eq!(f1["request_id"], "req-1");
    assert_eq!(f1["command"], "dispatch_next");

    assert_eq!(
        broadcaster.send(
            "proj-broadcast",
            slice_transitioned("req-1", "L4-Session-003")
        ),
        1
    );
    let f2: Value = serde_json::from_str(&read_text_frame(&mut sock).await).expect("json");
    assert_eq!(f2["event"], "slice.transitioned");
    assert_eq!(f2["slice_id"], "L4-Session-003");
    assert_eq!(f2["request_id"], "req-1");

    drop(sock);
    tx.send(()).ok();
    h.await.ok();
}

/// DSD-320 conformance — `command.accepted` must reach the issuing Session
/// within 100ms of the broadcaster send. We measure wall-clock from the
/// `send()` call to the moment the WS frame deserializes on the client side.
#[tokio::test]
async fn command_accepted_within_100ms() {
    let broadcaster = ProjectBroadcaster::new();
    let (addr, tx, h) = boot(broadcaster.clone()).await;
    let host = format!("{addr}");

    let mut sock = TcpStream::connect(addr).await.expect("connect");
    ws_handshake(&mut sock, "proj-timing", &host).await;

    // Same subscribe-warmup loop as the acceptance test — without it the
    // first send can race the upgrade task. The warmup event is itself a
    // valid command.accepted; we drain it before the timed leg.
    let warmup = loop {
        if broadcaster.send("proj-timing", cmd_accepted("warmup")) > 0 {
            break read_text_frame(&mut sock).await;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    let warmup_v: Value = serde_json::from_str(&warmup).expect("json");
    assert_eq!(warmup_v["request_id"], "warmup");

    let t0 = Instant::now();
    assert_eq!(broadcaster.send("proj-timing", cmd_accepted("hot")), 1);
    let raw = read_text_frame(&mut sock).await;
    let elapsed = t0.elapsed();
    let v: Value = serde_json::from_str(&raw).expect("json");
    assert_eq!(v["event"], "command_accepted");
    assert_eq!(v["request_id"], "hot");
    assert!(
        elapsed < Duration::from_millis(100),
        "DSD-320 violated: command.accepted took {elapsed:?}"
    );

    drop(sock);
    tx.send(()).ok();
    h.await.ok();
}

/// ISC-012 detection — Sessions are not durable; a new Session must not
/// see events that were emitted before it subscribed. Close A, open B,
/// emit a fresh event, assert B sees the fresh one and *only* the fresh
/// one (the pre-close event must not replay).
#[tokio::test]
async fn session_b_sees_no_replay_after_a_closed() {
    let broadcaster = ProjectBroadcaster::new();
    let (addr, tx, h) = boot(broadcaster.clone()).await;
    let host = format!("{addr}");

    // --- Session A leg ---
    let mut sock_a = TcpStream::connect(addr).await.expect("connect a");
    ws_handshake(&mut sock_a, "proj-replay", &host).await;
    // Warmup so we know A is subscribed.
    loop {
        if broadcaster.send("proj-replay", cmd_accepted("warmup")) > 0 {
            let _ = read_text_frame(&mut sock_a).await;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // Pre-close event that B must NOT see replayed.
    assert_eq!(
        broadcaster.send("proj-replay", cmd_accepted("pre-close")),
        1
    );
    let pre: Value = serde_json::from_str(&read_text_frame(&mut sock_a).await).expect("json");
    assert_eq!(pre["request_id"], "pre-close");

    drop(sock_a);

    // Wait for A's seat to release so B can acquire it (ISC-015).
    // The Drop of SessionLock fires from the spawned upgrade task; give it
    // a few ticks. We can't poll the registry without leaking it, so we
    // just retry the WS handshake until it stops 409'ing.
    let mut sock_b = None;
    for _ in 0..40 {
        let s = TcpStream::connect(addr).await.expect("connect b probe");
        let req = format!(
            "GET /projects/proj-replay/session HTTP/1.1\r\n\
             Host: {host}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
             Sec-WebSocket-Version: 13\r\n\
             \r\n"
        );
        let mut s = s;
        s.write_all(req.as_bytes()).await.expect("handshake write");
        let mut buf = vec![0u8; 1024];
        let n = s.read(&mut buf).await.unwrap_or(0);
        let head = String::from_utf8_lossy(&buf[..n]);
        if head.starts_with("HTTP/1.1 101") {
            sock_b = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let mut sock_b = sock_b.expect("session B failed to take the seat after A closed");

    // Nothing should be sitting in B's queue.
    assert!(
        try_read_text_frame(&mut sock_b, Duration::from_millis(50))
            .await
            .is_none(),
        "B saw a replayed event from before it subscribed (ISC-012 violated)"
    );

    // Now emit a *new* event; B sees it, proving the subscription is live
    // and the prior absence of replay wasn't because B wasn't subscribed.
    assert_eq!(
        broadcaster.send("proj-replay", cmd_accepted("post-open")),
        1
    );
    let post: Value = serde_json::from_str(&read_text_frame(&mut sock_b).await).expect("json");
    assert_eq!(post["request_id"], "post-open");

    drop(sock_b);
    tx.send(()).ok();
    h.await.ok();
}
