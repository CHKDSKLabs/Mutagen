//! L6-Release-001 — G3 / ISC-004 symmetric-exclusion release gate.
//!
//! A service holds the Project Lock; a CLI invocation against the same
//! project MUST exit `EXIT_LOCK_HELD` (78) and emit the DSD §2.3 tone
//! line — `lock held by service:<session_id>`. The CLI surface that
//! enforces lock acquisition lands in a follow-up slice (tracked in
//! project_state.md § Sessions — "cli_origin() helper not yet wired
//! into apply_state_update_for_slice / finalize_slice / resume_slice /
//! transition_active_slice"). Until then this test stays `#[ignore]`
//! so CI is honest about what it verifies; the slice exists as the
//! release-criteria gate that flips on once the wiring lands.
//!
//! Run manually:
//!   cargo test -p mutagen-core --test cli_service_exclusion -- --ignored

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use mutagen_core::project::lock::{EXIT_LOCK_HELD, LockHolder, acquire};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn fresh_project_root(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "mutagen-cli-svc-exclusion-{}-{}-{}-{}",
        tag,
        std::process::id(),
        nanos,
        seq
    ));
    std::fs::create_dir_all(&dir).expect("mkdir tempdir");
    dir
}

fn cli_manifest() -> PathBuf {
    // mutagen-core lives at harness/core/. Repo root is two parents up;
    // the CLI manifest is harness/cli/Cargo.toml from there.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("cli").join("Cargo.toml"))
        .expect("locate cli manifest")
}

/// G3 release-criteria detector. Foreign service holds the lock; CLI
/// must fail closed with EXIT_LOCK_HELD and the DSD §2.3 PROJECT_LOCKED
/// message format naming the live holder.
#[test]
#[ignore = "release-criteria gate: un-ignore once CLI lock acquisition wires through transition_active_slice / apply_state_update_for_slice"]
fn cli_exits_78_with_documented_message_when_service_holds_lock() {
    let root = fresh_project_root("locked");
    let session_id = "release-gate-session";
    let _guard = acquire(
        &root,
        LockHolder::Service {
            session_id: session_id.into(),
        },
    )
    .expect("foreign service acquire");

    let manifest = cli_manifest();
    let output = Command::new(env!("CARGO"))
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--")
        .arg("apply-state-update")
        .arg("--workspace-root")
        .arg(&root)
        .arg("--slice-id")
        .arg("L0-Release-Gate-000")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn mutagen-harness");

    let code = output.status.code().expect("CLI must surface an exit code");
    assert_eq!(
        code,
        EXIT_LOCK_HELD,
        "CLI must exit {EXIT_LOCK_HELD} when the lock is held — got {code}\n\
         stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let needle = format!("service:{session_id}");
    assert!(
        stderr.contains("lock held by") && stderr.contains(&needle),
        "CLI stderr must follow DSD §2.3 tone (`lock held by {needle}`); got: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&root);
}
