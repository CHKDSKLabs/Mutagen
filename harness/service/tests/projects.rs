//! Integration tests for `/projects` — FR-5, FR-6, ISC-008/010, DSD-300/311/312/621/627.
//!
//! Acceptance: cargo test -p mutagen-service --test projects
//! ISC-008 detection: relative_path_returns_422
//! INV-P3 detection:  duplicate_root_returns_409

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use mutagen_core::project::registry::ProjectRegistry;
use mutagen_service::observability;
use mutagen_service::routes::projects::{ProjectsState, router};
use serde_json::{Value, json};
use tokio::net::TcpListener;

fn tmp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nonce = format!(
        "mutagen-service-projects-{tag}-{pid}-{nanos}",
        pid = std::process::id(),
        nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    p.push(nonce);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn workspace_under(svc: &Path) -> PathBuf {
    let w = svc.join("workspace");
    std::fs::create_dir_all(&w).unwrap();
    w
}

async fn spawn(tag: &str) -> (SocketAddr, PathBuf, PathBuf) {
    let svc = tmp_dir(tag);
    let workspace = workspace_under(&svc);
    let registry_path = svc.join("projects.toml");
    let registry = ProjectRegistry::load(&registry_path).expect("load empty registry");
    let state = ProjectsState::new(registry);

    let app = observability::wrap(router(state));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, registry_path, workspace)
}

#[tokio::test]
async fn register_then_list_then_get_then_archive() {
    let (addr, _path, workspace) = spawn("happy").await;
    let client = reqwest::Client::new();

    let body = json!({ "root": workspace.to_string_lossy(), "name": "demo" });
    let resp = client
        .post(format!("http://{addr}/projects"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "POST /projects returns 201");
    let registered: Value = resp.json().await.unwrap();
    let pid = registered["project_id"].as_str().unwrap().to_string();
    assert_eq!(pid.len(), 36, "DSD-623: project_id is UUIDv7");
    assert_eq!(registered["status"], "registered");
    assert!(!registered["root"].as_str().unwrap().is_empty());

    let list_resp = client
        .get(format!("http://{addr}/projects"))
        .send()
        .await
        .unwrap();
    assert_eq!(list_resp.status(), 200);
    let list: Value = list_resp.json().await.unwrap();
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["project_id"], pid);

    let detail = client
        .get(format!("http://{addr}/projects/{pid}"))
        .send()
        .await
        .unwrap();
    assert_eq!(detail.status(), 200);
    let detail_body: Value = detail.json().await.unwrap();
    assert_eq!(detail_body["project_id"], pid);
    assert_eq!(detail_body["name"], "demo");

    let del = client
        .delete(format!("http://{addr}/projects/{pid}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), 204, "DELETE archives, returns 204");

    let after = client
        .get(format!("http://{addr}/projects/{pid}"))
        .send()
        .await
        .unwrap();
    assert_eq!(after.status(), 200, "archived projects are still readable");
    let after_body: Value = after.json().await.unwrap();
    assert_eq!(after_body["status"], "archived", "FR-6: soft archive");

    let ws_exists = workspace.exists();
    assert!(ws_exists, "FR-6: archiving must NOT delete workspace files");
}

#[tokio::test]
async fn empty_list_returns_items_array() {
    let (addr, _path, _ws) = spawn("emptylist").await;
    let resp = reqwest::get(format!("http://{addr}/projects"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().expect("items must be an array");
    assert!(items.is_empty(), "empty registry → items: []");
}

#[tokio::test]
async fn relative_path_returns_422() {
    // ISC-008 detection: a relative root MUST be rejected with 422 + DSD-624 envelope.
    let (addr, _path, _ws) = spawn("relpath").await;
    let body = json!({ "root": "relative/path", "name": "nope" });
    let resp = reqwest::Client::new()
        .post(format!("http://{addr}/projects"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422, "ISC-008: relative path is 422");
    let env: Value = resp.json().await.unwrap();
    assert_eq!(env["code"], "VALIDATION_FAILED");
    let details = env["details"]
        .as_array()
        .expect("details array per DSD-312");
    assert!(
        details
            .iter()
            .any(|d| d["field"] == "root" && d["rule"] == "absolute_path"),
        "details must name `root` and the `absolute_path` rule"
    );
}

#[tokio::test]
async fn duplicate_root_returns_409() {
    // INV-P3 detection: registering the same canonical root twice → 409.
    let (addr, _path, workspace) = spawn("dup").await;
    let body = json!({ "root": workspace.to_string_lossy(), "name": "first" });
    let r1 = reqwest::Client::new()
        .post(format!("http://{addr}/projects"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r1.status(), 201);

    let body2 = json!({ "root": workspace.to_string_lossy(), "name": "second" });
    let r2 = reqwest::Client::new()
        .post(format!("http://{addr}/projects"))
        .json(&body2)
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status(), 409, "INV-P3: duplicate canonical root → 409");
    let env: Value = r2.json().await.unwrap();
    assert_eq!(env["code"], "DUPLICATE_ROOT");
}

#[tokio::test]
async fn missing_required_fields_each_get_their_own_detail() {
    // DSD-312: one entry per missing field, not a concatenated message.
    let (addr, _path, _ws) = spawn("missing").await;
    let resp = reqwest::Client::new()
        .post(format!("http://{addr}/projects"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    let env: Value = resp.json().await.unwrap();
    let details = env["details"].as_array().unwrap();
    let fields: Vec<&str> = details.iter().filter_map(|d| d["field"].as_str()).collect();
    assert!(fields.contains(&"root"), "missing `root` entry");
    assert!(fields.contains(&"name"), "missing `name` entry");
    assert_eq!(fields.len(), 2, "DSD-312: one entry per missing field");
}

#[tokio::test]
async fn get_unknown_project_returns_404() {
    let (addr, _path, _ws) = spawn("notfound").await;
    let resp = reqwest::get(format!("http://{addr}/projects/not-a-real-id"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let env: Value = resp.json().await.unwrap();
    assert_eq!(env["code"], "PROJECT_NOT_FOUND");
}

#[tokio::test]
async fn delete_unknown_project_returns_404() {
    let (addr, _path, _ws) = spawn("delnotfound").await;
    let resp = reqwest::Client::new()
        .delete(format!("http://{addr}/projects/not-a-real-id"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let env: Value = resp.json().await.unwrap();
    assert_eq!(env["code"], "PROJECT_NOT_FOUND");
}

#[tokio::test]
async fn project_response_is_snake_case() {
    // DSD-621: every JSON field on the wire is snake_case.
    let (addr, _path, workspace) = spawn("snake").await;
    let body = json!({ "root": workspace.to_string_lossy(), "name": "snake" });
    let resp = reqwest::Client::new()
        .post(format!("http://{addr}/projects"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let raw: Value = resp.json().await.unwrap();
    for key in raw.as_object().unwrap().keys() {
        assert!(is_snake_case(key), "DSD-621: `{key}` is not snake_case");
    }
}

#[tokio::test]
async fn envelope_request_id_matches_response_header_no_inbound() {
    // DSD-624 / DSD-626: with no inbound x-request-id, the middleware mints a
    // UUIDv7 and the envelope must carry that exact value, not "".
    let (addr, _path, _ws) = spawn("rid-noheader").await;
    let resp = reqwest::Client::new()
        .post(format!("http://{addr}/projects"))
        .json(&json!({})) // 422 path
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    let header_rid = resp
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let env: Value = resp.json().await.unwrap();
    assert!(!header_rid.is_empty(), "middleware must mint a request id");
    assert_eq!(
        env["request_id"].as_str().unwrap_or(""),
        header_rid,
        "envelope.request_id must equal X-Request-Id header"
    );
}

#[tokio::test]
async fn envelope_request_id_matches_response_header_valid_uuidv7() {
    // Strict UUIDv7 inbound: middleware echoes; envelope must match.
    let (addr, _path, _ws) = spawn("rid-valid").await;
    let supplied = "0190f8b3-1d4d-7c4e-9abc-1234567890ab";
    let resp = reqwest::Client::new()
        .post(format!("http://{addr}/projects"))
        .header("x-request-id", supplied)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    let header_rid = resp
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(header_rid, supplied);
    let env: Value = resp.json().await.unwrap();
    assert_eq!(env["request_id"].as_str().unwrap(), supplied);
}

#[tokio::test]
async fn envelope_request_id_matches_response_header_garbage_inbound() {
    // Garbage inbound id: middleware mints a fresh UUIDv7 — envelope must carry
    // the minted value, not the rejected garbage.
    let (addr, _path, _ws) = spawn("rid-garbage").await;
    let resp = reqwest::Client::new()
        .post(format!("http://{addr}/projects"))
        .header("x-request-id", "not-a-uuidv7")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
    let header_rid = resp
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap()
        .to_string();
    assert_ne!(header_rid, "not-a-uuidv7", "garbage rejected by middleware");
    let env: Value = resp.json().await.unwrap();
    assert_eq!(env["request_id"].as_str().unwrap(), header_rid);
}

fn is_snake_case(s: &str) -> bool {
    !s.is_empty()
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
