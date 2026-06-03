//! L1-Infra-005 — structured logging + request_id propagation.
//! Includes the production observability code via #[path] because the slice
//! scope excludes lib.rs, so the module is not exposed through the lib crate.

// Standalone include of the production module — the route layer that reads
// RequestId/X_REQUEST_ID lives in the lib, not here, so those items look dead
// in this compilation view. They aren't; hush the lints for the test build.
#[allow(dead_code, unused_imports)]
#[path = "../src/observability/mod.rs"]
mod observability;

use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::routing::get;
use mutagen_service::app;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing_subscriber::fmt::MakeWriter;

use observability::{request_id::parse_uuid_v7_strict, wrap};

#[derive(Clone, Default)]
struct CaptureWriter {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl CaptureWriter {
    fn take(&self) -> String {
        let mut g = self.buf.lock().unwrap();
        let out = String::from_utf8_lossy(&g).into_owned();
        g.clear();
        out
    }
}

impl Write for CaptureWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for CaptureWriter {
    type Writer = CaptureWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
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
    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await
            .expect("serve");
    });
    (addr, tx, handle)
}

#[tokio::test]
async fn every_response_carries_request_id() {
    let (addr, tx, handle) = boot(wrap(app())).await;

    let resp = reqwest::get(format!("http://{addr}/health"))
        .await
        .expect("GET /health");

    let header = resp
        .headers()
        .get("x-request-id")
        .expect("x-request-id present")
        .to_str()
        .unwrap()
        .to_owned();
    assert!(
        parse_uuid_v7_strict(&header).is_some(),
        "generated id must be a v7 uuid, got {header}"
    );

    tx.send(()).ok();
    handle.await.ok();
}

#[tokio::test]
async fn supplied_request_id_is_honored() {
    let (addr, tx, handle) = boot(wrap(app())).await;

    // a valid v7 uuid — version nibble at the 13th char is '7'
    let supplied = "017f22e2-79b0-7cc3-98c4-dc0c0c07398f";

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/health"))
        .header("x-request-id", supplied)
        .send()
        .await
        .expect("send");

    let echoed = resp
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(echoed, supplied, "supplied id must be echoed verbatim");

    tx.send(()).ok();
    handle.await.ok();
}

#[tokio::test]
async fn malformed_request_id_is_replaced() {
    let (addr, tx, handle) = boot(wrap(app())).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/health"))
        .header("x-request-id", "not-a-uuid")
        .send()
        .await
        .expect("send");

    let echoed = resp
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();
    assert_ne!(echoed, "not-a-uuid");
    assert!(parse_uuid_v7_strict(echoed).is_some());

    tx.send(()).ok();
    handle.await.ok();
}

#[tokio::test]
async fn secret_field_names_filtered() {
    let writer = CaptureWriter::default();
    let subscriber = observability::build_subscriber("info", writer.clone());

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(secret = "shhhh", "secret field event");
        tracing::info!(token = "abc123", "token field event");
        tracing::info!(bearer = "xyz", "bearer field event");
        tracing::info!(authorization = "Bearer xyz", "authz field event");
        tracing::info!(plain = "ok", "plain event survives");
    });

    let captured = writer.take();
    for forbidden in observability::FORBIDDEN_FIELD_NAMES {
        assert!(
            !captured.contains(&format!("\"{forbidden}\"")),
            "found forbidden field name {forbidden} in capture:\n{captured}"
        );
    }
    assert!(
        captured.contains("plain event survives"),
        "non-forbidden event was filtered: {captured}"
    );
}

#[tokio::test]
async fn request_id_span_carries_required_fields() {
    let writer = CaptureWriter::default();
    let subscriber = observability::build_subscriber("info", writer.clone());

    let captured = {
        // Install the capture subscriber as the *global* default rather than a
        // thread-local `set_default`. The request is served on a task spawned
        // by `boot`, so a thread-local dispatcher only reaches it by luck of
        // the current-thread runtime sharing the test's thread — and the
        // per-callsite Interest cache can be pinned to `Never` by a sibling
        // test that hits the same callsite under the no-op global first.
        // `set_global_default` is visible on every worker thread and rebuilds
        // the interest cache, so capture is deterministic. The unique `/probe`
        // path keeps sibling requests that also reach the global writer from
        // affecting the assertions below.
        let dispatch = tracing::Dispatch::new(subscriber);
        tracing::dispatcher::set_global_default(dispatch)
            .expect("global subscriber installs once per test binary");

        let router = wrap(Router::new().route("/probe", get(|| async { "pong" })));
        let (addr, tx, handle) = boot(router).await;
        let _ = reqwest::get(format!("http://{addr}/probe"))
            .await
            .expect("probe");
        tx.send(()).ok();
        handle.await.ok();
        writer.take()
    };

    // Span fields show up in the JSON output via the formatter. The request.completed
    // event itself carries status + latency_ms + request_id as event fields.
    assert!(
        captured.contains("request.completed"),
        "missing request.completed event: {captured}"
    );
    assert!(
        captured.contains("\"status\":200"),
        "status field missing: {captured}"
    );
    assert!(
        captured.contains("\"latency_ms\""),
        "latency_ms field missing: {captured}"
    );
    assert!(
        captured.contains("\"request_id\""),
        "request_id field missing: {captured}"
    );
    assert!(
        captured.contains("\"method\":\"GET\""),
        "method span field missing: {captured}"
    );
    assert!(
        captured.contains("\"path\":\"/probe\""),
        "path span field missing: {captured}"
    );
}

#[test]
fn uuid_v7_validator_rejects_non_v7() {
    // version nibble 4 → must reject (this is a v4)
    assert!(parse_uuid_v7_strict("f47ac10b-58cc-4372-a567-0e02b2c3d479").is_none());
    // uppercase → reject
    assert!(parse_uuid_v7_strict("017F22E2-79B0-7CC3-98C4-DC0C0C07398F").is_none());
    // too short → reject
    assert!(parse_uuid_v7_strict("017f22e2").is_none());
}

#[test]
fn uuid_v7_generator_passes_strict_validator() {
    for _ in 0..32 {
        let id = observability::request_id::generate_uuid_v7();
        assert!(
            parse_uuid_v7_strict(&id).is_some(),
            "generator produced {id}"
        );
    }
}
