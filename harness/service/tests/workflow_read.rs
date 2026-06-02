//! Integration tests for /projects/{id}/{status,slices,state-log}.
//!
//! Acceptance: cargo test -p mutagen-service --test workflow_read
//! ISC-006 detection: state_log_paginates_with_opaque_cursor
//! ISC-007 detection: state_log_surfaces_cli_origin_records
//! ISC-010 detection: dto_fields_are_snake_case_on_the_wire
//! DSD-625 detection: state_log_paginates_with_opaque_cursor (cursor token only)

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use mutagen_core::project::registry::ProjectRegistry;
use mutagen_service::observability;
use mutagen_service::routes::projects::ProjectsState;
use mutagen_service::routes::workflow_read;
use serde_json::{Value, json};
use tokio::net::TcpListener;

fn tmp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nonce = format!(
        "mutagen-workflow-read-{tag}-{pid}-{nanos}",
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

fn write_queue(root: &Path) {
    let q = json!({
        "version": 1,
        "generated_at": "2026-05-12T00:00:00Z",
        "generated_by": "test",
        "pipeline_mode": "full",
        "planning_advisories": [],
        "slices": [
            {
                "id": "L1-Foo-001",
                "title": "first slice",
                "status": "pending",
                "author_agent": "Bebop",
                "layer": 1,
                "bounded_context": "Foo",
                "target_loc": 100,
                "objective": "do the thing"
            },
            {
                "id": "L1-Foo-002",
                "title": "second slice",
                "status": "completed",
                "author_agent": "Bebop",
                "layer": 1,
                "bounded_context": "Foo",
                "target_loc": 100,
                "objective": "do the other thing"
            }
        ]
    });
    let dir = root.join("slices");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("queue.json"),
        serde_json::to_vec_pretty(&q).unwrap(),
    )
    .unwrap();
}

fn write_active(root: &Path) {
    let a = json!({
        "slice_id": "L1-Foo-001",
        "title": "first slice",
        "evidence_bundle_path": ".mutagen/state/evidence/L1-Foo-001.md",
        "author_agent": "Bebop",
        "active_agent": "Bebop",
        "stage": "author",
        "pipeline_mode": "full",
        "review_required": true,
        "layer": 1,
        "bounded_context": "Foo",
        "context_to_update": "project_state.md",
        "context_file": "project_state.md",
        "attempts": 1,
        "max_retries": 3,
        "micro_corrections_used": 0,
        "max_micro_corrections": 2,
        "allowed_write_globs": [],
        "host": "stub"
    });
    let dir = root.join(".mutagen/state");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("active-slice.json"),
        serde_json::to_vec_pretty(&a).unwrap(),
    )
    .unwrap();
}

fn write_log(root: &Path, lines: &[Value]) {
    let dir = root.join(".mutagen/state");
    std::fs::create_dir_all(&dir).unwrap();
    let mut body = String::new();
    for line in lines {
        body.push_str(&serde_json::to_string(line).unwrap());
        body.push('\n');
    }
    std::fs::write(dir.join("log.jsonl"), body).unwrap();
}

async fn spawn(tag: &str) -> (SocketAddr, String, PathBuf) {
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

    let app = observability::wrap(workflow_read::router(state));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, project_id, workspace)
}

#[tokio::test]
async fn status_returns_counts_and_active_slice() {
    let (addr, pid, root) = spawn("status").await;
    write_queue(&root);
    write_active(&root);

    let resp = reqwest::get(format!("http://{addr}/projects/{pid}/status"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["project_id"], pid);
    assert_eq!(body["pipeline_mode"], "full");
    assert_eq!(body["active_slice_id"], "L1-Foo-001");
    assert_eq!(body["active_stage"], "author");
    assert_eq!(body["total_slices"], 2);
    assert_eq!(body["slice_counts"]["pending"], 1);
    assert_eq!(body["slice_counts"]["completed"], 1);
}

#[tokio::test]
async fn status_missing_project_returns_404() {
    let (addr, _pid, _root) = spawn("status404").await;
    let resp = reqwest::get(format!("http://{addr}/projects/does-not-exist/status"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "PROJECT_NOT_FOUND");
}

#[tokio::test]
async fn slices_lists_queue_entries() {
    let (addr, pid, root) = spawn("slices").await;
    write_queue(&root);

    let resp = reqwest::get(format!("http://{addr}/projects/{pid}/slices"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let items = body["slices"].as_array().expect("slices is an array");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], "L1-Foo-001");
    assert_eq!(items[0]["status"], "pending");
    assert_eq!(items[1]["status"], "completed");
}

#[tokio::test]
async fn slices_empty_queue_returns_empty_list() {
    let (addr, pid, _root) = spawn("emptyq").await;
    let resp = reqwest::get(format!("http://{addr}/projects/{pid}/slices"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let items = body["slices"].as_array().unwrap();
    assert!(items.is_empty());
}

#[tokio::test]
async fn state_log_surfaces_cli_origin_records() {
    // ISC-007 detection: a CLI-authored record (origin.kind=cli, id=<pid>)
    // round-trips through the read endpoint with the correct shape.
    let (addr, pid, root) = spawn("clilog").await;
    write_log(
        &root,
        &[json!({
            "schema_version": 1,
            "slice_id": "L1-Foo-001",
            "event": "slice.transitioned",
            "at": "2026-05-12T00:00:01Z",
            "origin": { "kind": "cli", "pid": 4242 }
        })],
    );

    let resp = reqwest::get(format!("http://{addr}/projects/{pid}/state-log"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["origin"]["kind"], "cli");
    assert_eq!(items[0]["origin"]["id"], "4242");
    assert_eq!(items[0]["schema_version"], 1);
    assert!(body["next_cursor"].is_null());
}

#[tokio::test]
async fn state_log_paginates_with_opaque_cursor() {
    // DSD-625 detection: pagination is cursor-only; offset is forbidden.
    // The first call returns next_cursor, the second resumes at that cursor.
    let (addr, pid, root) = spawn("paginate").await;
    let lines: Vec<Value> = (0..5)
        .map(|i| {
            json!({
                "schema_version": 1,
                "slice_id": format!("L1-Foo-{i:03}"),
                "event": "slice.transitioned",
                "at": format!("2026-05-12T00:00:{:02}Z", i),
                "origin": { "kind": "service", "session_id": format!("sess-{i}") }
            })
        })
        .collect();
    write_log(&root, &lines);

    let r1 = reqwest::get(format!("http://{addr}/projects/{pid}/state-log?limit=2"))
        .await
        .unwrap();
    let p1: Value = r1.json().await.unwrap();
    assert_eq!(p1["items"].as_array().unwrap().len(), 2);
    let cursor = p1["next_cursor"].as_str().expect("next_cursor present");

    let r2 = reqwest::get(format!(
        "http://{addr}/projects/{pid}/state-log?limit=2&cursor={cursor}"
    ))
    .await
    .unwrap();
    let p2: Value = r2.json().await.unwrap();
    assert_eq!(p2["items"].as_array().unwrap().len(), 2);
    assert_eq!(p2["items"][0]["slice_id"], "L1-Foo-002");
    let cursor2 = p2["next_cursor"].as_str().unwrap();

    let r3 = reqwest::get(format!(
        "http://{addr}/projects/{pid}/state-log?limit=2&cursor={cursor2}"
    ))
    .await
    .unwrap();
    let p3: Value = r3.json().await.unwrap();
    assert_eq!(p3["items"].as_array().unwrap().len(), 1);
    assert!(p3["next_cursor"].is_null(), "tail reports no cursor");
}

#[tokio::test]
async fn state_log_rejects_malformed_cursor_with_422() {
    // DSD-625 / DSD-627: opaque cursors are server-minted. A gibberish cursor
    // (transport mangling, client bug, poisoning attempt) MUST NOT silently
    // rewind to start=0 — that hands the caller a duplicate first page and
    // poisons their dedupe story. Mirrors the existing cursor>EOF 422 path.
    let (addr, pid, root) = spawn("badcursor").await;
    write_log(
        &root,
        &[json!({
            "schema_version": 1,
            "slice_id": "L1-Foo-001",
            "event": "slice.transitioned",
            "at": "2026-05-12T00:00:00Z",
            "origin": { "kind": "cli", "pid": 1 }
        })],
    );

    for garbage in ["zzzz", "0xff", "%20", "not-hex"] {
        let resp = reqwest::get(format!(
            "http://{addr}/projects/{pid}/state-log?cursor={garbage}"
        ))
        .await
        .unwrap();
        assert_eq!(
            resp.status(),
            422,
            "cursor={garbage:?} should be rejected, got {}",
            resp.status()
        );
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["code"], "INVALID_CURSOR");
        assert!(
            body["items"].is_null(),
            "422 body is an error envelope, not a page"
        );
    }
}

#[tokio::test]
async fn state_log_tolerates_pre_0_4_0_records() {
    // ISC-006 / MD-4: legacy records (no schema_version, no origin) are
    // tolerated by replay and surface here with origin: null.
    let (addr, pid, root) = spawn("legacy").await;
    write_log(
        &root,
        &[json!({
            "slice_id": "L0-old",
            "event": "slice.transitioned",
            "at": "2026-04-01T00:00:00Z"
        })],
    );
    let resp = reqwest::get(format!("http://{addr}/projects/{pid}/state-log"))
        .await
        .unwrap();
    let body: Value = resp.json().await.unwrap();
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert!(items[0]["origin"].is_null(), "legacy record has no origin");
    assert!(items[0]["schema_version"].is_null());
}

#[tokio::test]
async fn state_log_missing_log_returns_empty_page() {
    let (addr, pid, _root) = spawn("nolog").await;
    let resp = reqwest::get(format!("http://{addr}/projects/{pid}/state-log"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert!(body["items"].as_array().unwrap().is_empty());
    assert!(body["next_cursor"].is_null());
}

#[tokio::test]
async fn dto_fields_are_snake_case_on_the_wire() {
    // ISC-010 / DSD-621: handler returns DTOs, not domain types, and field
    // casing is snake_case across status / slices / state-log.
    let (addr, pid, root) = spawn("snake").await;
    write_queue(&root);
    write_active(&root);
    write_log(
        &root,
        &[json!({
            "schema_version": 1,
            "slice_id": "L1-Foo-001",
            "event": "slice.transitioned",
            "at": "2026-05-12T00:00:00Z",
            "origin": { "kind": "cli", "pid": 1 }
        })],
    );

    let s: Value = reqwest::get(format!("http://{addr}/projects/{pid}/status"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    for key in [
        "project_id",
        "pipeline_mode",
        "active_slice_id",
        "active_stage",
        "slice_counts",
        "total_slices",
        "last_state_update_at",
    ] {
        assert!(s.get(key).is_some(), "status missing snake_case key {key}");
    }
    for key in [
        "pending",
        "in_progress",
        "blocked_retry",
        "completed",
        "escalated",
        "refused",
    ] {
        assert!(
            s["slice_counts"].get(key).is_some(),
            "slice_counts missing {key}"
        );
    }

    let q: Value = reqwest::get(format!("http://{addr}/projects/{pid}/slices"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let entry = &q["slices"][0];
    for key in [
        "id",
        "title",
        "status",
        "layer",
        "bounded_context",
        "author_agent",
        "attempts",
        "target_loc",
        "objective",
    ] {
        assert!(
            entry.get(key).is_some(),
            "slice missing snake_case key {key}"
        );
    }

    let l: Value = reqwest::get(format!("http://{addr}/projects/{pid}/state-log"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let item = &l["items"][0];
    for key in ["slice_id", "event", "at", "origin", "schema_version"] {
        assert!(item.get(key).is_some(), "state-log item missing key {key}");
    }
}
