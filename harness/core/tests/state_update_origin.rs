//! ISC-006 / ISC-007 acceptance suite for the State Update log.
//!
//! Covers:
//! - CLI + service origin records round-trip through write → replay.
//! - A post-0.4.0 record with `schema_version` but no `origin` fails replay.
//! - A truncated log fails replay closed against the persisted offset.
//! - The Origin constructor rejects empty session ids (type-level ISC-007).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use mutagen_core::project::lock::{LockGuard, LockHolder, acquire};
use mutagen_core::workflow::origin::{Origin, OriginError};
use mutagen_core::workflow::replay::replay;
use mutagen_core::workflow::state_update::{StateUpdate, append_record, log_path, offset_path};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Each test gets a fresh project root under the OS tmpdir. The dirname
/// folds in pid + time + a process-local counter so parallel test threads
/// can't collide.
fn fresh_project_root(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "mutagen-state-update-{}-{}-{}-{}",
        tag,
        std::process::id(),
        nanos,
        seq
    ));
    fs::create_dir_all(&dir).expect("create project root");
    dir
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}

fn lock_for(root: &Path, tag: &str) -> LockGuard {
    acquire(
        root,
        LockHolder::Service {
            session_id: format!("test-{tag}"),
        },
    )
    .unwrap_or_else(|e| panic!("acquire project lock: {e}"))
}

fn now_iso() -> String {
    // We don't depend on chrono in this crate, so build a sortable stamp
    // from the unix epoch. The replay does not interpret `at`, just preserves it.
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("1970-01-01T00:00:00.{nanos:030}Z")
}

#[test]
fn cli_and_service_records_round_trip() {
    let root = fresh_project_root("happy");
    let guard = lock_for(&root, "happy");

    let cli_record = StateUpdate::new(
        "L2-Workflow-002",
        "slice.transitioned",
        now_iso(),
        Origin::cli(4242),
    );
    let svc_record = StateUpdate::new(
        "L2-Workflow-002",
        "slice.transitioned",
        now_iso(),
        Origin::service("sess-abc-123").expect("non-empty session id"),
    );

    let size_after_cli = append_record(&guard, &root, &cli_record).expect("append cli");
    let size_after_svc = append_record(&guard, &root, &svc_record).expect("append svc");
    assert!(
        size_after_svc > size_after_cli,
        "log grew after second append"
    );

    let records = replay(&root).expect("replay clean log");
    assert_eq!(records.len(), 2, "two records replayed: {records:?}");
    assert_eq!(
        records[0].origin,
        Some(Origin::cli(4242)),
        "first record carries cli origin"
    );
    assert_eq!(
        records[1].origin,
        Some(Origin::service("sess-abc-123").unwrap()),
        "second record carries service origin"
    );
    assert_eq!(records[0].schema_version, Some(1));
    assert_eq!(records[1].schema_version, Some(1));

    drop(guard);
    cleanup(&root);
}

#[test]
fn service_origin_constructor_rejects_empty_session_id() {
    assert!(matches!(
        Origin::service(""),
        Err(OriginError::EmptySessionId)
    ));
    assert!(matches!(
        Origin::service("   "),
        Err(OriginError::EmptySessionId)
    ));
}

#[test]
fn pre_0_4_0_records_without_origin_are_tolerated() {
    // Simulate a 0.3.x log: no schema_version, no origin. MD-4 says we
    // must keep these readable so existing installations can upgrade.
    let root = fresh_project_root("legacy");
    let path = log_path(&root);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let legacy =
        r#"{"slice_id":"L1-old-001","event":"slice.transitioned","at":"2026-04-01T00:00:00Z"}"#;
    fs::write(&path, format!("{legacy}\n")).unwrap();

    let records = replay(&root).expect("pre-0.4.0 records replay clean");
    assert_eq!(records.len(), 1);
    assert!(
        records[0].origin.is_none(),
        "legacy record has no origin and we tolerate that"
    );
    assert!(
        records[0].schema_version.is_none(),
        "legacy record has no schema_version"
    );

    cleanup(&root);
}

#[test]
fn post_0_4_0_record_missing_origin_fails_replay() {
    let root = fresh_project_root("missing-origin");
    let path = log_path(&root);
    fs::create_dir_all(path.parent().unwrap()).unwrap();

    // Hand-craft a corrupted post-0.4.0 record: it declares schema_version
    // but omits origin. This shouldn't ever exist in nature because the
    // type-level writer makes it impossible to produce — but corruption
    // and editing happen, and replay is the second line of defense.
    let bad = r#"{"schema_version":1,"slice_id":"L2-bad","event":"slice.transitioned","at":"2026-05-09T00:00:00Z"}"#;
    fs::write(&path, format!("{bad}\n")).unwrap();

    let err = replay(&root).expect_err("replay must fail closed");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("missing") && (msg.contains("origin") || msg.contains("ISC-007")),
        "error must mention missing origin: {msg}"
    );

    cleanup(&root);
}

#[test]
fn truncated_log_fails_closed() {
    let root = fresh_project_root("truncated");
    let guard = lock_for(&root, "truncated");

    let rec = StateUpdate::new(
        "L2-Workflow-002",
        "slice.transitioned",
        now_iso(),
        Origin::cli(99),
    );
    append_record(&guard, &root, &rec).expect("append");
    append_record(&guard, &root, &rec).expect("append second");

    // Stash the offset, then chop the log down — exactly the scenario
    // ISC-006's append-only invariant exists to detect.
    let path = log_path(&root);
    fs::write(&path, b"").expect("truncate log");

    let err = replay(&root).expect_err("replay must fail closed after truncation");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("shrank") || msg.contains("recovery"),
        "error must mention shrinkage / manual recovery: {msg}"
    );

    drop(guard);
    cleanup(&root);
}

#[test]
fn cli_writes_origin_cli_pid() {
    // ISC-007 detection vector: a CLI-authored State Update lands on disk
    // with origin.kind=cli and origin.pid=<current process id>. The CLI helper
    // (`harness/cli/src/commands/state_log.rs::cli_origin`) folds
    // std::process::id() into the Cli variant; the writer then serializes that
    // through the regular append path so replay sees it intact.
    let root = fresh_project_root("cli-pid");
    let guard = lock_for(&root, "cli-pid");
    let pid = std::process::id();

    let record = StateUpdate::new(
        "L4-Workflow-001",
        "slice.transitioned",
        now_iso(),
        Origin::cli(pid),
    );
    append_record(&guard, &root, &record).expect("append cli record");

    let records = replay(&root).expect("replay clean log");
    assert_eq!(records.len(), 1, "exactly one record on disk");
    assert_eq!(
        records[0].origin,
        Some(Origin::Cli { pid }),
        "record carries cli:<current-pid> origin"
    );
    let raw = fs::read_to_string(log_path(&root)).expect("read log");
    assert!(
        raw.contains("\"kind\":\"cli\"") && raw.contains(&format!("\"pid\":{pid}")),
        "raw JSONL surfaces snake_case `kind:cli` and the literal pid: {raw}"
    );

    drop(guard);
    cleanup(&root);
}

#[test]
fn offset_file_is_written_after_each_append() {
    let root = fresh_project_root("offset");
    let guard = lock_for(&root, "offset");
    let rec = StateUpdate::new(
        "L2-Workflow-002",
        "slice.transitioned",
        now_iso(),
        Origin::cli(1),
    );
    let size = append_record(&guard, &root, &rec).expect("append");
    let stored = fs::read_to_string(offset_path(&root)).expect("read offset");
    assert_eq!(stored.trim().parse::<u64>().unwrap(), size);

    drop(guard);
    cleanup(&root);
}
