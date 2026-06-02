//! L3-Auth-002 — integration tests for the bearer-auth middleware.
//!
//! Pulls the production modules in via `#[path]` because the slice scope
//! does not widen the lib's public surface (mirrors the pattern used by
//! `tests/observability.rs`).

#[path = "../src/auth/allowlist.rs"]
mod allowlist;
#[path = "../src/auth/middleware.rs"]
mod middleware;
#[path = "../src/auth/outcome.rs"]
mod outcome;

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use mutagen_service::auth::Secret;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

use middleware::{auth_wrap, verify_request};

const KNOWN_SECRET: &str = "open-sesame-rosebud-supercalafragilistic";

fn fixture_secret() -> Arc<Secret> {
    Arc::new(Secret::new(
        KNOWN_SECRET.as_bytes().to_vec(),
        "test:fixture".to_owned(),
    ))
}

fn test_router(secret: Arc<Secret>) -> Router {
    let inner = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/version", get(|| async { "v" }))
        .route("/openapi.json", get(|| async { "{}" }))
        .route("/projects", get(|| async { "projects" }))
        .route("/projects/{id}", get(|| async { "project" }))
        .route("/slices", get(|| async { "slices" }));
    auth_wrap(inner, secret)
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

#[tokio::test]
async fn allowlist_routes_pass_without_secret() {
    let (addr, tx, h) = boot(test_router(fixture_secret())).await;
    for path in ["/health", "/version", "/openapi.json"] {
        let resp = reqwest::get(format!("http://{addr}{path}"))
            .await
            .expect("send");
        assert_eq!(
            resp.status(),
            200,
            "allowlisted {path} must pass without auth"
        );
    }
    tx.send(()).ok();
    h.await.ok();
}

#[tokio::test]
async fn non_allowlisted_route_rejects_without_secret() {
    let (addr, tx, h) = boot(test_router(fixture_secret())).await;
    for path in ["/projects", "/projects/abc", "/slices"] {
        let resp = reqwest::get(format!("http://{addr}{path}"))
            .await
            .expect("send");
        assert_eq!(
            resp.status(),
            401,
            "non-allowlisted {path} must reject sans Authorization"
        );
    }
    tx.send(()).ok();
    h.await.ok();
}

#[tokio::test]
async fn correct_bearer_unlocks_non_allowlisted_route() {
    let (addr, tx, h) = boot(test_router(fixture_secret())).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/projects"))
        .header("authorization", format!("Bearer {KNOWN_SECRET}"))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    tx.send(()).ok();
    h.await.ok();
}

#[tokio::test]
async fn wrong_bearer_still_rejects() {
    let (addr, tx, h) = boot(test_router(fixture_secret())).await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/projects"))
        .header("authorization", "Bearer not-the-real-secret")
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 401);
    tx.send(()).ok();
    h.await.ok();
}

#[tokio::test]
async fn auth_401_envelope_does_not_leak_reason() {
    let (addr, tx, h) = boot(test_router(fixture_secret())).await;
    let client = reqwest::Client::new();

    let missing = reqwest::get(format!("http://{addr}/projects"))
        .await
        .expect("send");
    let wrong = client
        .get(format!("http://{addr}/projects"))
        .header("authorization", "Bearer canary-marker-bytes")
        .send()
        .await
        .expect("send");
    let wrong_scheme = client
        .get(format!("http://{addr}/projects"))
        .header("authorization", "Basic dXNlcjpwYXNz")
        .send()
        .await
        .expect("send");
    let malformed = client
        .get(format!("http://{addr}/projects"))
        .header("authorization", "garbage-no-scheme")
        .send()
        .await
        .expect("send");

    let bodies: Vec<Value> = vec![
        missing.json().await.expect("json"),
        wrong.json().await.expect("json"),
        wrong_scheme.json().await.expect("json"),
        malformed.json().await.expect("json"),
    ];

    let baseline_code = bodies[0]["code"].as_str().expect("code").to_owned();
    let baseline_message = bodies[0]["message"].as_str().expect("message").to_owned();
    assert_eq!(baseline_code, "UNAUTHENTICATED");

    for b in &bodies {
        assert_eq!(b["code"].as_str().expect("code"), baseline_code);
        assert_eq!(b["message"].as_str().expect("message"), baseline_message);

        // INV-A4: nothing in the body distinguishes which check failed.
        for leaky in ["reason", "scheme", "header", "detail"] {
            assert!(b.get(leaky).is_none(), "envelope leaked field {leaky}: {b}");
        }
        if let Some(d) = b.get("details") {
            let empty = d.is_null() || d.as_object().map(|o| o.is_empty()).unwrap_or(false);
            assert!(empty, "details field must not carry a reason: {d}");
        }

        // ISC-001: bytes from the offending header MUST NOT echo back.
        let text = b.to_string();
        assert!(!text.contains("canary-marker-bytes"));
        assert!(!text.contains("dXNlcjpwYXNz"));
        assert!(!text.contains("garbage-no-scheme"));
    }

    tx.send(()).ok();
    h.await.ok();
}

#[tokio::test]
async fn error_envelope_shape_matches_dsd_624() {
    let (addr, tx, h) = boot(test_router(fixture_secret())).await;
    let resp = reqwest::get(format!("http://{addr}/projects"))
        .await
        .expect("send");
    assert_eq!(resp.status(), 401);
    let v: Value = resp.json().await.expect("json");

    let code = v["code"].as_str().expect("code is string");
    let message = v["message"].as_str().expect("message is string");
    let request_id = v["request_id"].as_str().expect("request_id present");

    assert!(
        code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
        "code must be SCREAMING_SNAKE_CASE per DSD-624, got {code}"
    );
    assert_eq!(
        message,
        message.to_lowercase(),
        "message must be lowercase per DSD-624"
    );
    // request_id may be empty when the client didn't supply one (no info to echo);
    // the response header carries the canonical id per DSD-626. Just assert presence.
    let _ = request_id;

    let allowed_keys = ["code", "message", "request_id", "details"];
    if let Some(obj) = v.as_object() {
        for k in obj.keys() {
            assert!(
                allowed_keys.contains(&k.as_str()),
                "envelope has unexpected field {k}"
            );
        }
    } else {
        panic!("envelope must be an object: {v}");
    }

    tx.send(()).ok();
    h.await.ok();
}

#[tokio::test]
async fn supplied_request_id_echoes_into_envelope() {
    let (addr, tx, h) = boot(test_router(fixture_secret())).await;
    let client = reqwest::Client::new();
    let supplied = "017f22e2-79b0-7cc3-98c4-dc0c0c07398f";
    let resp = client
        .get(format!("http://{addr}/projects"))
        .header("x-request-id", supplied)
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 401);
    let v: Value = resp.json().await.expect("json");
    assert_eq!(v["request_id"].as_str().unwrap_or(""), supplied);
    tx.send(()).ok();
    h.await.ok();
}

#[test]
fn allowlist_constant_matches_dsd_303() {
    assert_eq!(
        allowlist::UNAUTHENTICATED,
        &["/health", "/version", "/openapi.json"]
    );
}

#[test]
fn verify_request_is_outcome_not_bool() {
    let secret = Secret::new(b"hunter2".to_vec(), "test:fixed".to_owned());

    // ISC-003 surface check: the only "good" arm is the explicit Accept on a
    // successful comparison. Every other branch produces a Reject — there is
    // no Result::unwrap_or(true) shaped escape hatch.
    match verify_request(None, &secret) {
        outcome::AuthOutcome::Reject(_) => {}
        outcome::AuthOutcome::Accept(_) => panic!("missing header must reject"),
    }
    match verify_request(Some(b"Bearer hunter2"), &secret) {
        outcome::AuthOutcome::Accept(_) => {}
        outcome::AuthOutcome::Reject(_) => panic!("good secret must accept"),
    }
    match verify_request(Some(b"Bearer wrong"), &secret) {
        outcome::AuthOutcome::Reject(_) => {}
        outcome::AuthOutcome::Accept(_) => panic!("wrong secret must reject"),
    }
}
