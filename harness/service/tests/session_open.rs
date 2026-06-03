//! Integration tests for the WebSocket upgrade route — L4-Session-001.
//!
//! Acceptance: cargo test -p mutagen-service --test session_open
//! ISC-015 detection: second_session_returns_409
//! NFR-2 detection:    ws_handshake_rejects_without_auth
//! DSD-634 conformance: session_span_carries_session_id_and_project_id

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use mutagen_service::auth::Secret;
use mutagen_service::observability;
use mutagen_service::routes::session::{SessionState, session_router};
use mutagen_service::session::ActiveSessionRegistry;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

// Pull the middleware in the same way auth_middleware.rs does — the auth
// wrap module isn't part of the public lib API.
#[path = "../src/auth/allowlist.rs"]
mod allowlist;
#[path = "../src/auth/middleware.rs"]
mod middleware;
#[path = "../src/auth/outcome.rs"]
mod outcome;

use middleware::auth_wrap;

const TEST_SECRET: &str = "open-sesame-rosebud-supercalafragilistic";

fn fixture_secret() -> Arc<Secret> {
    Arc::new(Secret::new(
        TEST_SECRET.as_bytes().to_vec(),
        "test:fixture".to_owned(),
    ))
}

fn fresh_state() -> SessionState {
    SessionState::new(ActiveSessionRegistry::new(), "secret:test:fixture")
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

/// Hand-rolled WebSocket upgrade request. Returns the live TCP socket so the
/// caller can either read the handshake response and close, or hold it open
/// while a second client tries to barge in (ISC-015 case).
fn handshake_request(
    project_id: &str,
    bearer: Option<&str>,
    schema_version: Option<&str>,
    host: &str,
) -> Vec<u8> {
    let mut path = format!("/projects/{project_id}/session");
    if let Some(v) = schema_version {
        path.push_str(&format!("?schema_version={v}"));
    }
    let mut req = String::new();
    req.push_str(&format!("GET {path} HTTP/1.1\r\n"));
    req.push_str(&format!("Host: {host}\r\n"));
    req.push_str("Upgrade: websocket\r\n");
    req.push_str("Connection: Upgrade\r\n");
    req.push_str("Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n");
    req.push_str("Sec-WebSocket-Version: 13\r\n");
    if let Some(b) = bearer {
        req.push_str(&format!("Authorization: Bearer {b}\r\n"));
    }
    req.push_str("\r\n");
    req.into_bytes()
}

/// Parsed HTTP/1.1 response head — status code and the body bytes after
/// `\r\n\r\n`. Body is whatever was buffered when the read completed.
struct HandshakeReply {
    status: u16,
    headers: String,
    body: Vec<u8>,
}

async fn read_reply(socket: &mut TcpStream) -> HandshakeReply {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let read = tokio::time::timeout_at(deadline, socket.read(&mut chunk))
            .await
            .expect("read timeout")
            .expect("read");
        if read == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..read]);
        if let Some(end) = find_header_end(&buf) {
            // For non-101 responses we want the body too. Look for content-length
            // and keep reading until we have it.
            let headers = String::from_utf8_lossy(&buf[..end]).to_string();
            let content_length = parse_content_length(&headers).unwrap_or(0);
            let body_start = end + 4;
            while buf.len() < body_start + content_length {
                let n = tokio::time::timeout_at(deadline, socket.read(&mut chunk))
                    .await
                    .expect("body timeout")
                    .expect("body read");
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let status = parse_status(&headers).expect("status line");
            let body = buf[body_start..].to_vec();
            return HandshakeReply {
                status,
                headers,
                body,
            };
        }
    }
    panic!(
        "socket closed without a complete header block, got {} bytes",
        buf.len()
    );
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_status(headers: &str) -> Option<u16> {
    headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
}

fn parse_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let mut parts = line.splitn(2, ':');
        if let (Some(k), Some(v)) = (parts.next(), parts.next())
            && k.eq_ignore_ascii_case("content-length")
        {
            return v.trim().parse().ok();
        }
    }
    None
}

#[tokio::test]
async fn session_open() {
    // Acceptance: a properly-formed upgrade against an unclaimed project gets
    // 101 Switching Protocols.
    let state = fresh_state();
    let (addr, tx, h) = boot(observability::wrap(session_router(state))).await;
    let host = format!("{addr}");
    let mut socket = TcpStream::connect(addr).await.expect("connect");
    socket
        .write_all(&handshake_request("proj-happy", None, None, &host))
        .await
        .expect("write");
    let reply = read_reply(&mut socket).await;
    assert_eq!(
        reply.status, 101,
        "happy path must upgrade. headers:\n{}",
        reply.headers
    );
    assert!(
        reply.headers.to_lowercase().contains("upgrade: websocket"),
        "must announce websocket upgrade. headers:\n{}",
        reply.headers
    );
    drop(socket);
    tx.send(()).ok();
    h.await.ok();
}

#[tokio::test]
async fn second_session_returns_409() {
    // ISC-015: second concurrent upgrade on same project_id MUST be 409 and
    // MUST carry the active session_id in details.
    let state = fresh_state();
    let registry = Arc::clone(&state.registry);
    let (addr, tx, h) = boot(observability::wrap(session_router(state))).await;
    let host = format!("{addr}");

    let mut sock_a = TcpStream::connect(addr).await.expect("connect a");
    sock_a
        .write_all(&handshake_request("proj-dup", None, None, &host))
        .await
        .expect("write a");

    // Drain A's reply head so we know A is seated. Keep sock_a alive in scope
    // so the server doesn't see EOF and release A's seat before B's attempt.
    let reply_a = read_reply(&mut sock_a).await;
    assert_eq!(reply_a.status, 101, "A must upgrade");

    // A's seat should now be visible in the registry.
    let active = registry.is_active("proj-dup");
    assert!(active.is_some(), "registry must hold proj-dup after A");

    // B attempts on same project_id while A is still connected.
    let mut sock_b = TcpStream::connect(addr).await.expect("connect b");
    sock_b
        .write_all(&handshake_request("proj-dup", None, None, &host))
        .await
        .expect("write b");
    let reply_b = read_reply(&mut sock_b).await;

    assert_eq!(
        reply_b.status,
        409,
        "B must get 409. headers:\n{}\nbody: {}",
        reply_b.headers,
        String::from_utf8_lossy(&reply_b.body)
    );
    let body: Value = serde_json::from_slice(&reply_b.body).expect("envelope JSON");
    assert_eq!(body["code"], "SESSION_CONFLICT");
    let active_id = body["details"]["active_session_id"]
        .as_str()
        .expect("active_session_id present");
    assert_eq!(
        active_id,
        active.as_deref().unwrap_or(""),
        "409 must quote A's session_id"
    );

    drop(sock_a);
    drop(sock_b);
    tx.send(()).ok();
    h.await.ok();
}

#[tokio::test]
async fn ws_handshake_rejects_without_auth() {
    // NFR-2 / ISC-011: auth runs BEFORE the protocol switch — a missing
    // bearer must yield a plain 401, not an open WebSocket.
    let state = fresh_state();
    let router = observability::wrap(auth_wrap(session_router(state), fixture_secret()));
    let (addr, tx, h) = boot(router).await;
    let host = format!("{addr}");

    let mut socket = TcpStream::connect(addr).await.expect("connect");
    socket
        .write_all(&handshake_request("proj-noauth", None, None, &host))
        .await
        .expect("write");
    let reply = read_reply(&mut socket).await;

    assert_eq!(
        reply.status, 401,
        "unauthenticated upgrade must reject. headers:\n{}",
        reply.headers
    );
    let upgrade_header = reply.headers.to_lowercase();
    assert!(
        !upgrade_header.contains("upgrade: websocket"),
        "401 response MUST NOT advertise a websocket upgrade. headers:\n{}",
        reply.headers
    );
    drop(socket);

    // Sanity: with a good bearer, the same router accepts the upgrade.
    let mut socket2 = TcpStream::connect(addr).await.expect("connect2");
    socket2
        .write_all(&handshake_request(
            "proj-noauth",
            Some(TEST_SECRET),
            None,
            &host,
        ))
        .await
        .expect("write2");
    let reply2 = read_reply(&mut socket2).await;
    assert_eq!(
        reply2.status, 101,
        "good bearer must upgrade. headers:\n{}",
        reply2.headers
    );
    drop(socket2);

    tx.send(()).ok();
    h.await.ok();
}

#[tokio::test]
async fn session_span_carries_session_id_and_project_id() {
    // DSD-634: the `session` span emits session_id + project_id + principal_id
    // on every log line below it.
    use std::io::Write;
    use std::sync::Mutex;
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct Capture {
        buf: Arc<Mutex<Vec<u8>>>,
    }
    impl Capture {
        fn take(&self) -> String {
            let mut g = self.buf.lock().unwrap();
            let out = String::from_utf8_lossy(&g).into_owned();
            g.clear();
            out
        }
    }
    impl Write for Capture {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.buf.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> MakeWriter<'a> for Capture {
        type Writer = Capture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    let writer = Capture::default();
    let subscriber = observability::build_subscriber("info", writer.clone());
    // Install as the *global* default rather than a thread-local `set_default`:
    // session.opened is emitted from the WebSocket upgrade future, which axum
    // runs on a spawned task. A thread-local dispatcher only reaches that task
    // by luck of the current-thread runtime sharing the test's thread, and the
    // per-callsite Interest cache can be pinned to `Never` by a sibling test
    // first — which is what made this flaky in CI. The global default is
    // visible on every worker thread and rebuilds the interest cache. The
    // unique `proj-span` project id keeps sibling session events that also
    // reach the global writer from affecting the assertions below.
    let dispatch = tracing::Dispatch::new(subscriber);
    tracing::dispatcher::set_global_default(dispatch)
        .expect("global subscriber installs once per test binary");

    let state = fresh_state();
    let router = observability::wrap(session_router(state));
    let (addr, tx, h) = boot(router).await;
    let host = format!("{addr}");

    let mut socket = TcpStream::connect(addr).await.expect("connect");
    socket
        .write_all(&handshake_request("proj-span", None, None, &host))
        .await
        .expect("write");
    let reply = read_reply(&mut socket).await;
    assert_eq!(reply.status, 101, "upgrade must succeed for log capture");
    drop(socket);

    // Give the spawned span-emitting task a tick to flush before we tear down.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let captured = writer.take();
    assert!(
        captured.contains("session.opened"),
        "expected session.opened event, got:\n{captured}"
    );
    assert!(
        captured.contains("\"session_id\""),
        "session_id field missing:\n{captured}"
    );
    assert!(
        captured.contains("\"project_id\":\"proj-span\""),
        "project_id field missing or wrong:\n{captured}"
    );
    assert!(
        captured.contains("\"principal_id\""),
        "principal_id field missing:\n{captured}"
    );

    tx.send(()).ok();
    h.await.ok();
}
