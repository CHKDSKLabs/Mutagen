//! L6-Release-001 — G3 cross-project isolation.
//!
//! Two simultaneous Sessions on two distinct projects MUST produce
//! independent elicitation checkpoints. Backs NFR-4 (single-project
//! crash blast radius) and the Project aggregate's per-root state-dir
//! discipline (DDD §3.2). No append from project β must leak into
//! project α's elicitation.jsonl, and vice versa.
//!
//! Hand-rolled WS framing, same minimal client as session_chat.rs —
//! the protocol surface this test exercises is one masked client text
//! frame in, one unmasked server text frame out, repeat.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use mutagen_service::observability;
use mutagen_service::routes::session::{ProjectRootResolver, SessionState, session_router};
use mutagen_service::session::ActiveSessionRegistry;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

struct MultiRoot(HashMap<String, PathBuf>);
impl ProjectRootResolver for MultiRoot {
    fn resolve(&self, project_id: &str) -> Option<PathBuf> {
        self.0.get(project_id).cloned()
    }
}

static SUFFIX: AtomicU64 = AtomicU64::new(0);

fn tempdir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "mutagen-multiproj-{tag}-{pid}-{n}",
        pid = std::process::id(),
        n = SUFFIX.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).expect("mkdir tempdir");
    p
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
    socket.write_all(req.as_bytes()).await.expect("hs write");
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 256];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let n = tokio::time::timeout_at(deadline, socket.read(&mut chunk))
            .await
            .expect("hs timeout")
            .expect("hs read");
        if n == 0 {
            panic!("EOF before WS handshake completed");
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    assert!(
        String::from_utf8_lossy(&buf).starts_with("HTTP/1.1 101"),
        "expected 101 upgrade"
    );
}

async fn send_text_frame(socket: &mut TcpStream, payload: &str) {
    let bytes = payload.as_bytes();
    let mut frame = vec![0x81u8];
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
            .expect("read timeout")
            .expect("read");
        if n == 0 {
            panic!("EOF after {filled}/{} bytes", out.len());
        }
        filled += n;
    }
}

async fn read_text_frame(socket: &mut TcpStream) -> String {
    let mut header = [0u8; 2];
    read_exact(socket, &mut header).await;
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
    assert_eq!(header[0] & 0x0F, 0x1, "expected text frame");
    String::from_utf8(payload).expect("utf-8")
}

async fn drive_turn(addr: std::net::SocketAddr, project_id: &str, answer: &str) -> String {
    let host = format!("{addr}");
    let mut sock = TcpStream::connect(addr).await.expect("connect");
    ws_handshake(&mut sock, project_id, &host).await;
    send_text_frame(
        &mut sock,
        &json!({ "op": "issue_question", "prompt": "pick", "kind": "free_text" }).to_string(),
    )
    .await;
    let issued: Value = serde_json::from_str(&read_text_frame(&mut sock).await).unwrap();
    let qid = issued["envelope"]["question_id"]
        .as_str()
        .unwrap()
        .to_owned();
    send_text_frame(
        &mut sock,
        &json!({
            "op": "submit_answer",
            "question_id": qid,
            "answer": { "kind": "free_text", "text": answer },
        })
        .to_string(),
    )
    .await;
    let _ = read_text_frame(&mut sock).await;
    drop(sock);
    qid
}

/// NFR-4 / DDD §3.2: each project's elicitation log holds only its own
/// turns. Two Sessions, two roots, two distinct answers — neither file
/// must contain the other project's reply.
#[tokio::test]
async fn two_sessions_two_projects_keep_separate_elicitation_logs() {
    let root_a = tempdir("a");
    let root_b = tempdir("b");
    let mut map = HashMap::new();
    map.insert("proj-alpha".to_string(), root_a.clone());
    map.insert("proj-beta".to_string(), root_b.clone());

    let state = SessionState::new(ActiveSessionRegistry::new(), "secret:test:multiproj")
        .with_root_resolver(Arc::new(MultiRoot(map)));
    let router = observability::wrap(session_router(state));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let (tx, rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .expect("serve");
    });

    // Concurrent turns — both must land independently, no interleaving
    // corruption, no cross-contamination.
    let qid_a = drive_turn(addr, "proj-alpha", "ALPHA_REPLY").await;
    let qid_b = drive_turn(addr, "proj-beta", "BETA_REPLY").await;

    // Server takes a tick to flush each FR-13 append.
    tokio::time::sleep(Duration::from_millis(150)).await;

    let log_a = std::fs::read_to_string(root_a.join(".mutagen/state/elicitation.jsonl"))
        .expect("alpha log");
    let log_b =
        std::fs::read_to_string(root_b.join(".mutagen/state/elicitation.jsonl")).expect("beta log");

    assert!(
        log_a.contains("ALPHA_REPLY"),
        "alpha log missing its reply: {log_a}"
    );
    assert!(
        log_b.contains("BETA_REPLY"),
        "beta log missing its reply: {log_b}"
    );
    assert!(
        !log_a.contains("BETA_REPLY"),
        "alpha log leaked beta's reply: {log_a}"
    );
    assert!(
        !log_b.contains("ALPHA_REPLY"),
        "beta log leaked alpha's reply: {log_b}"
    );
    assert!(
        !log_a.contains(&qid_b),
        "alpha log carries beta's question_id {qid_b}: {log_a}"
    );
    assert!(
        !log_b.contains(&qid_a),
        "beta log carries alpha's question_id {qid_a}: {log_b}"
    );
    assert_eq!(
        log_a.lines().filter(|l| !l.trim().is_empty()).count(),
        1,
        "alpha log must hold exactly its own turn"
    );
    assert_eq!(
        log_b.lines().filter(|l| !l.trim().is_empty()).count(),
        1,
        "beta log must hold exactly its own turn"
    );

    tx.send(()).ok();
    server.await.ok();

    let _ = std::fs::remove_dir_all(&root_a);
    let _ = std::fs::remove_dir_all(&root_b);
}
