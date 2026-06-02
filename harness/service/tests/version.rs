//! Integration tests for `GET /version` (L4-Edge-001).
//!
//! Covers FR-4a (unauthenticated version probe), INV-E4 / ISC-010 (handler
//! returns only DTO types), DSD-621 (snake_case wire shape), DSD-626 (every
//! response carries `X-Request-Id`).

use std::future::Future;
use std::net::SocketAddr;

use axum::Json;
use mutagen_service::dto::VersionDto;
use mutagen_service::{app, observability, routes};
use tokio::net::TcpListener;

async fn spawn() -> SocketAddr {
    let router = observability::wrap(app());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap_or_else(|e| {
        panic!("test listener failed to bind: {e}");
    });
    let addr = listener.local_addr().unwrap_or_else(|e| {
        panic!("test listener has no local addr: {e}");
    });
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    addr
}

#[tokio::test]
async fn returns_200_and_correct_shape() {
    let addr = spawn().await;
    let url = format!("http://{addr}/version");

    let resp = reqwest::get(&url)
        .await
        .unwrap_or_else(|e| panic!("GET /version failed: {e}"));

    assert_eq!(resp.status(), 200, "FR-4a: /version must return 200");

    let request_id = resp
        .headers()
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    assert!(
        request_id.is_some(),
        "DSD-626: response must carry X-Request-Id"
    );
    assert_eq!(
        request_id.as_deref().map(str::len),
        Some(36),
        "X-Request-Id should be a UUIDv7 (36 chars)"
    );

    let body: VersionDto = resp
        .json()
        .await
        .unwrap_or_else(|e| panic!("decoding VersionDto: {e}"));

    assert_eq!(body.service_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(body.harness_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(body.chat_protocol_schema_version, "1");
}

#[tokio::test]
async fn unauthenticated_request_succeeds() {
    // ISC-011: /version is allowlisted. We hit it with no Authorization header
    // (the test router doesn't even mount auth) and expect 200 either way.
    let addr = spawn().await;
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/version"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET /version failed: {e}"));
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn response_body_is_snake_case() {
    // DSD-621: every JSON field MUST be snake_case.
    let addr = spawn().await;
    let resp = reqwest::get(format!("http://{addr}/version"))
        .await
        .unwrap_or_else(|e| panic!("GET /version failed: {e}"));
    let raw: serde_json::Value = resp
        .json()
        .await
        .unwrap_or_else(|e| panic!("decoding JSON: {e}"));
    let obj = raw
        .as_object()
        .unwrap_or_else(|| panic!("/version body must be a JSON object"));
    for key in obj.keys() {
        assert!(
            is_snake_case(key),
            "DSD-621 violation: field `{key}` is not snake_case"
        );
    }
}

#[tokio::test]
async fn handler_returns_only_dto_types() {
    // INV-E4 / ISC-010 compile-time gate: if `get_version` ever returns
    // something that isn't `Json<crate::dto::VersionDto>`, this test stops
    // compiling. Runtime body is just a smoke check that the signature
    // produces a JSON object — the real teeth are in the type bounds.
    fn assert_returns_version_dto<F, Fut>(_f: F)
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Json<VersionDto>>,
    {
    }
    assert_returns_version_dto(routes::version::get_version);

    let dto = routes::version::get_version().await;
    assert_eq!(dto.chat_protocol_schema_version, "1");
}

fn is_snake_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let bytes = s.as_bytes();
    if !bytes[0].is_ascii_lowercase() {
        return false;
    }
    bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
}
