//! Integration tests for POST /projects/{id}/(dispatch-next | slices/{id}/...).
//!
//! Acceptance: cargo test -p mutagen-service --test workflow_write
//! ISC-005 detection: second_writer_gets_409
//! ISC-006 detection: state_update_appended_with_service_origin
//! ISC-007 detection: state_update_appended_with_service_origin (origin field present)
//! DSD-330 detection: escalate_requires_confirmation_token
//! DSD-322 detection: dispatch_next_returns_202_envelope (visible confirmation)

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use mutagen_core::project::lock::{self, LockHolder};
use mutagen_core::project::registry::ProjectRegistry;
use mutagen_service::observability;
use mutagen_service::routes::projects::ProjectsState;
use mutagen_service::routes::workflow_write::{self, WorkflowWriteState};
use serde_json::{Value, json};
use tokio::net::TcpListener;

fn tmp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nonce = format!(
        "mutagen-workflow-write-{tag}-{pid}-{nanos}",
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

async fn spawn(tag: &str) -> (SocketAddr, String, PathBuf, WorkflowWriteState) {
    let svc = tmp_dir(tag);
    let workspace = svc.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let registry_path = svc.join("projects.toml");
    let mut registry = ProjectRegistry::load(&registry_path).expect("load empty registry");
    let entry = registry
        .register("demo", &workspace)
        .expect("register project");
    let project_id = entry.project_id.clone();
    let state = ProjectsState::new(registry);
    let write_state = WorkflowWriteState::new(state);

    let app = observability::wrap(workflow_write::router(write_state.clone()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, project_id, workspace, write_state)
}

async fn spawn_two(tag: &str) -> (SocketAddr, String, PathBuf, String, PathBuf) {
    // Two projects sharing one registry / one router so a token minted
    // against α can be replayed at β over the same listener. Mirrors the
    // Tiger Claw adversarial reproducer for the cross-project token leak.
    let svc = tmp_dir(tag);
    let alpha_ws = svc.join("alpha");
    let beta_ws = svc.join("beta");
    std::fs::create_dir_all(&alpha_ws).unwrap();
    std::fs::create_dir_all(&beta_ws).unwrap();
    let registry_path = svc.join("projects.toml");
    let mut registry = ProjectRegistry::load(&registry_path).expect("load empty registry");
    let alpha = registry
        .register("alpha", &alpha_ws)
        .expect("register alpha");
    let alpha_id = alpha.project_id.clone();
    let beta = registry.register("beta", &beta_ws).expect("register beta");
    let beta_id = beta.project_id.clone();
    let state = ProjectsState::new(registry);
    let write_state = WorkflowWriteState::new(state);

    let app = observability::wrap(workflow_write::router(write_state));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, alpha_id, alpha_ws, beta_id, beta_ws)
}

fn read_log(root: &Path) -> Vec<Value> {
    let p = root.join(".mutagen/state/log.jsonl");
    let Ok(raw) = std::fs::read_to_string(&p) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

#[tokio::test]
async fn dispatch_next_returns_202_envelope() {
    // DSD-322: silent success forbidden; 202 carries CommandAcceptedDto.
    let (addr, pid, root, _state) = spawn("dispatch").await;
    let resp = reqwest::Client::new()
        .post(format!("http://{addr}/projects/{pid}/dispatch-next"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["command"], "dispatch_next");
    assert_eq!(body["project_id"], pid);
    assert!(body["slice_id"].is_null(), "dispatch_next has no slice yet");
    assert!(!body["request_id"].as_str().unwrap().is_empty());
    assert!(!body["accepted_at"].as_str().unwrap().is_empty());

    let log = read_log(&root);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["event"], "cohort.dispatched");
}

#[tokio::test]
async fn state_update_appended_with_service_origin() {
    // ISC-006 / ISC-007: append-only writer used; origin.kind=service.
    let (addr, pid, root, _s) = spawn("origin").await;
    let resp = reqwest::Client::new()
        .post(format!(
            "http://{addr}/projects/{pid}/slices/L1-Foo-001/accept"
        ))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202);
    let log = read_log(&root);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0]["slice_id"], "L1-Foo-001");
    assert_eq!(log[0]["event"], "slice.transitioned");
    assert_eq!(log[0]["origin"]["kind"], "service");
    let sid = log[0]["origin"]["session_id"].as_str().unwrap();
    assert_eq!(sid.len(), 36, "service origin id is the uuidv7 request id");
}

#[tokio::test]
async fn second_writer_gets_409() {
    // ISC-005 detection: foreign lock holder makes any Workflow Command
    // 409 PROJECT_LOCKED with the holder identity surfaced.
    let (addr, pid, root, _s) = spawn("locked").await;
    let _foreign = lock::acquire(
        &root,
        LockHolder::Service {
            session_id: "foreign-session".into(),
        },
    )
    .expect("foreign acquire");

    let resp = reqwest::Client::new()
        .post(format!("http://{addr}/projects/{pid}/dispatch-next"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 409);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "PROJECT_LOCKED");
    let holder = body["details"]["holder"].as_str().unwrap();
    assert!(
        holder.contains("foreign-session"),
        "409 envelope must name the lock holder, got {holder}"
    );
}

#[tokio::test]
async fn escalate_requires_confirmation_token() {
    // DSD-330 detection: escalate without a token is 422; with a fresh
    // token from confirm-escalate it is 202.
    let (addr, pid, root, _s) = spawn("escalate").await;
    let client = reqwest::Client::new();

    let no_token = client
        .post(format!(
            "http://{addr}/projects/{pid}/slices/L1-Foo-001/escalate"
        ))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(no_token.status(), 422);
    let body: Value = no_token.json().await.unwrap();
    assert_eq!(body["code"], "CONFIRMATION_REQUIRED");

    let bogus = client
        .post(format!(
            "http://{addr}/projects/{pid}/slices/L1-Foo-001/escalate"
        ))
        .json(&json!({ "confirmation_token": "not-a-real-token" }))
        .send()
        .await
        .unwrap();
    assert_eq!(bogus.status(), 422);

    let mint = client
        .post(format!(
            "http://{addr}/projects/{pid}/slices/L1-Foo-001/confirm-escalate"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(mint.status(), 200);
    let token = mint.json::<Value>().await.unwrap()["confirmation_token"]
        .as_str()
        .unwrap()
        .to_string();

    let ok = client
        .post(format!(
            "http://{addr}/projects/{pid}/slices/L1-Foo-001/escalate"
        ))
        .json(&json!({ "confirmation_token": token, "reason": "human review" }))
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 202);
    let log = read_log(&root);
    assert!(log.iter().any(|r| r["event"] == "workflow.escalated"));

    // Single-use: token cannot be reused.
    let replay = client
        .post(format!(
            "http://{addr}/projects/{pid}/slices/L1-Foo-001/escalate"
        ))
        .json(&json!({ "confirmation_token": token }))
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), 422);
}

#[tokio::test]
async fn confirm_escalate_token_is_slice_scoped() {
    // DSD-330: a token minted for slice A cannot escalate slice B.
    let (addr, pid, _root, _s) = spawn("scoped").await;
    let client = reqwest::Client::new();
    let mint = client
        .post(format!(
            "http://{addr}/projects/{pid}/slices/L1-Foo-001/confirm-escalate"
        ))
        .send()
        .await
        .unwrap();
    let token = mint.json::<Value>().await.unwrap()["confirmation_token"]
        .as_str()
        .unwrap()
        .to_string();

    let cross = client
        .post(format!(
            "http://{addr}/projects/{pid}/slices/L1-Foo-002/escalate"
        ))
        .json(&json!({ "confirmation_token": token }))
        .send()
        .await
        .unwrap();
    assert_eq!(cross.status(), 422);
}

#[tokio::test]
async fn escalate_token_must_be_project_scoped() {
    // DSD-330 / DSD-331 regression. The token's identity is (project_id,
    // slice_id) — a mint on α's slice X cannot authorize β's slice X.
    let (addr, alpha_id, _alpha_root, beta_id, beta_root) = spawn_two("xproj").await;
    let client = reqwest::Client::new();

    let mint = client
        .post(format!(
            "http://{addr}/projects/{alpha_id}/slices/L1-Foo-001/confirm-escalate"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(mint.status(), 200);
    let token = mint.json::<Value>().await.unwrap()["confirmation_token"]
        .as_str()
        .unwrap()
        .to_string();

    let cross = client
        .post(format!(
            "http://{addr}/projects/{beta_id}/slices/L1-Foo-001/escalate"
        ))
        .json(&json!({ "confirmation_token": token }))
        .send()
        .await
        .unwrap();
    assert_eq!(cross.status(), 422, "cross-project consume must fail");
    let body: Value = cross.json().await.unwrap();
    assert_eq!(body["code"], "CONFIRMATION_REQUIRED");

    // β's log stays silent — the rejected destructive command must not
    // leave any State Update behind (ISC-006).
    let beta_log = read_log(&beta_root);
    assert!(
        beta_log.iter().all(|r| r["event"] != "workflow.escalated"),
        "beta got a workflow.escalated record from a foreign token: {beta_log:?}"
    );
}

#[tokio::test]
async fn finalize_and_resume_write_state_updates() {
    let (addr, pid, root, _s) = spawn("finres").await;
    let client = reqwest::Client::new();
    let f = client
        .post(format!(
            "http://{addr}/projects/{pid}/slices/L1-Foo-001/finalize"
        ))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(f.status(), 202);
    let r = client
        .post(format!(
            "http://{addr}/projects/{pid}/slices/L1-Foo-001/resume"
        ))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 202);

    let log = read_log(&root);
    assert_eq!(log.len(), 2);
    // Both commands produce slice.transitioned events; the DDD §3.1 from→to
    // detail rides in the body when the future state machine adds it.
    for rec in &log {
        assert_eq!(rec["event"], "slice.transitioned");
        assert_eq!(rec["origin"]["kind"], "service");
    }
}

#[tokio::test]
async fn unknown_project_returns_404() {
    let (addr, _pid, _root, _s) = spawn("nopid").await;
    let resp = reqwest::Client::new()
        .post(format!(
            "http://{addr}/projects/does-not-exist/dispatch-next"
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "PROJECT_NOT_FOUND");
}
