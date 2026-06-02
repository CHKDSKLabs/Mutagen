// 2026-05-06 L1-Infra-005 postmortem action item #2: finalize hard-gates on
// the buildable verifier (default `cargo check`) for any slice whose
// write_set carries `.rs` files. These tests inject stub closures via
// `finalize_slice_with` to drive the gate without paying cargo's startup
// tax — same shape as the L1-Harness-002 missing-artifact suite.

use mutagen_core::adapter::HostKind;
use mutagen_core::finalize::{FinalizeSliceOptions, finalize_slice_with};
use mutagen_core::queue::{BishopVerdict, KaraiStructuralVerdict, SliceStatus, TigerClawVerdict};
use mutagen_core::queue_update::{UpdateSliceOptions, update_slice};
use mutagen_core::runtime::{PrepareNextOptions, prepare_next};
use mutagen_core::state::Stage;
use mutagen_core::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

const SLICE_ID: &str = "L1-orders-001";

#[test]
fn finalize_refuses_when_verifier_errs() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.rewrite_write_set(&["src/orders/aggregate.rs", "tests/orders/aggregate_test.rs"]);
    workspace.seed_cargo_root();
    workspace.prepare_claimed_slice();
    workspace.seed_author_artifacts();
    workspace.write_author_output(SLICE_ID, &author_output_body());
    workspace.write_text("project_state.md", "# Project State\n");
    workspace.set_clean_verdicts();
    workspace.transition_to_state_record();

    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let verifier = move |_manifest: &Path| -> anyhow::Result<()> {
        counter.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("error[E0425]: cannot find value `nope` in this scope")
    };

    let error = finalize_slice_with(workspace.finalize_options(false), &verifier)
        .expect_err("finalize must refuse when the buildable check fails");

    let message = error.to_string();
    assert!(
        message.contains("non-compiling Rust"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("E0425"),
        "verifier error should be surfaced: {message}"
    );
    assert!(calls.load(Ordering::SeqCst) >= 1, "verifier should run");

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "finalization_failed");
    assert!(
        queue["slices"][0]["escalation_reason"]
            .as_str()
            .unwrap_or_default()
            .contains("buildable_check_failed"),
        "escalation_reason should record the structured reason: {}",
        queue["slices"][0]["escalation_reason"]
    );

    assert!(
        workspace
            .root
            .join(".mutagen/state/active-slice.json")
            .exists(),
        "blocked finalize should leave the active slice in place"
    );
    assert!(
        !workspace
            .root
            .join(".mutagen/state/finalize-audit.jsonl")
            .exists(),
        "non-override path must not emit an audit record"
    );
}

#[test]
fn finalize_accepts_when_verifier_ok() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.rewrite_write_set(&["src/orders/aggregate.rs", "tests/orders/aggregate_test.rs"]);
    workspace.seed_cargo_root();
    workspace.prepare_claimed_slice();
    workspace.seed_author_artifacts();
    workspace.write_author_output(SLICE_ID, &author_output_body());
    workspace.write_text("project_state.md", "# Project State\n");
    workspace.set_clean_verdicts();
    workspace.transition_to_state_record();

    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let verifier = move |_manifest: &Path| -> anyhow::Result<()> {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    };

    let result = finalize_slice_with(workspace.finalize_options(false), &verifier)
        .expect("finalize must succeed when the verifier returns Ok");

    assert_eq!(result.status, SliceStatus::Completed);
    assert!(result.broken_build_manifests.is_empty());
    assert!(!result.broken_build_overridden);
    assert!(calls.load(Ordering::SeqCst) >= 1, "verifier should run");
}

#[test]
fn finalize_skips_gate_when_no_rust_files() {
    // The write_set is the default basic_ready globs — no entry ends in
    // `.rs` — so the verifier must never be called and finalize sails through.
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_claimed_slice();
    workspace.seed_author_artifacts();
    workspace.write_author_output(SLICE_ID, &author_output_body());
    workspace.write_text("project_state.md", "# Project State\n");
    workspace.set_clean_verdicts();
    workspace.transition_to_state_record();

    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let verifier = move |_manifest: &Path| -> anyhow::Result<()> {
        counter.fetch_add(1, Ordering::SeqCst);
        anyhow::bail!("should never run")
    };

    let result = finalize_slice_with(workspace.finalize_options(false), &verifier)
        .expect("finalize without .rs write_set entries should bypass the gate");

    assert_eq!(result.status, SliceStatus::Completed);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "verifier must not run when write_set has no .rs entries"
    );
}

#[test]
fn accept_broken_build_records_audit() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.rewrite_write_set(&["src/orders/aggregate.rs", "tests/orders/aggregate_test.rs"]);
    workspace.seed_cargo_root();
    workspace.prepare_claimed_slice();
    workspace.seed_author_artifacts();
    workspace.write_author_output(SLICE_ID, &author_output_body());
    workspace.write_text("project_state.md", "# Project State\n");
    workspace.set_clean_verdicts();
    workspace.transition_to_state_record();

    let verifier = |_manifest: &Path| -> anyhow::Result<()> {
        anyhow::bail!("error[E0432]: unresolved import")
    };

    let result = finalize_slice_with(workspace.finalize_options(true), &verifier)
        .expect("override should finalize even when the verifier complains");

    assert_eq!(result.status, SliceStatus::Completed);
    assert!(result.broken_build_overridden);
    assert!(!result.broken_build_manifests.is_empty());

    let audit_path = workspace.root.join(".mutagen/state/finalize-audit.jsonl");
    let audit_body = fs::read_to_string(&audit_path).expect("override should leave audit record");
    let first_line = audit_body
        .lines()
        .next()
        .expect("audit log must have a row");
    let entry: Value = serde_json::from_str(first_line).expect("audit row should parse");
    assert_eq!(entry["event"], "slice.finalize_broken_build_overridden");
    assert_eq!(entry["slice_id"], SLICE_ID);
    assert!(
        entry["broken_manifests"]
            .as_array()
            .map(|arr| !arr.is_empty())
            .unwrap_or(false),
        "audit record should carry the broken-manifest list"
    );
}

fn author_output_body() -> String {
    r#"### 🛠️ Execution: L1-orders-001
#### Intake Report
- Domain fit: standard execution ✓
#### Code Artifacts
- `src/orders/aggregate.rs`
- `tests/orders/aggregate_test.rs`
#### ISC Upholding Map
| ISC | Code site | Mechanism | Detection test |
#### Verification Artifacts
- Acceptance: cargo test
#### State Update
### L1-orders-001 — 2026-05-13
**Artifacts:** src/orders/aggregate.rs, tests/orders/aggregate_test.rs
"#
    .to_string()
}

struct FixtureWorkspace {
    root: PathBuf,
}

impl FixtureWorkspace {
    fn copy(name: &str) -> Self {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name);
        let destination = unique_temp_dir(name);

        copy_dir_recursive(&source, &destination).expect("fixture copy should succeed");

        let workspace = Self { root: destination };
        support::write_queue_validation(&workspace.root);
        workspace
    }

    /// Patch the slice's write_set in-place, then re-emit queue-validation.json
    /// so the contract hash matches the mutated body. Tests that need .rs
    /// entries in the write_set call this immediately after `copy`.
    fn rewrite_write_set(&self, entries: &[&str]) {
        let queue_path = self.root.join("slices/queue.json");
        let raw = fs::read_to_string(&queue_path).expect("queue should read");
        let mut queue: Value = serde_json::from_str(&raw).expect("queue should parse");
        let target = queue["slices"]
            .as_array_mut()
            .and_then(|slices| slices.iter_mut().find(|s| s["id"] == SLICE_ID))
            .expect("slice should exist in fixture");
        target["write_set"] =
            Value::Array(entries.iter().map(|e| Value::String((*e).into())).collect());

        let body = serde_json::to_string_pretty(&queue).expect("queue should serialize");
        fs::write(&queue_path, format!("{body}\n")).expect("queue should write");

        support::write_queue_validation(&self.root);
    }

    /// Drop a stub Cargo.toml at the workspace root so `nearest_cargo_manifest`
    /// has something to find when discover_rust_manifests walks upward from
    /// `src/orders/aggregate.rs`. The verifier we inject never reads it.
    fn seed_cargo_root(&self) {
        self.write_text(
            "Cargo.toml",
            "[package]\nname = \"fixture-stub\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        );
    }

    fn prepare_claimed_slice(&self) {
        prepare_next(PrepareNextOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            queue_validation_path: support::queue_validation_path(&self.root),
            workflow_config_path: self.root.join(".claude/workflow.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            host: HostKind::Codex,
            dry_run: false,
        })
        .expect("prepare-next should succeed");

        transition_active_slice(self.transition_options(Stage::Author, true))
            .expect("author transition should succeed");
    }

    fn seed_author_artifacts(&self) {
        self.write_text("src/orders/aggregate.rs", "// placeholder\n");
        self.write_text("tests/orders/aggregate_test.rs", "// placeholder\n");
    }

    fn set_clean_verdicts(&self) {
        update_slice(UpdateSliceOptions {
            queue_path: self.root.join("slices/queue.json"),
            slice_id: SLICE_ID.to_string(),
            status: None,
            attempts: None,
            micro_corrections_used: None,
            karai_structural: Some(KaraiStructuralVerdict::Pass),
            bishop: Some(BishopVerdict::Skip),
            tiger_claw: Some(TigerClawVerdict::Clean),
            micro_correction: Some(false),
            completed_at: None,
            clear_completed_at: false,
            escalation_reason: None,
            clear_escalation_reason: false,
            human_check_resolved_at: None,
            clear_human_check_resolved_at: false,
        })
        .expect("queue update should succeed");
    }

    fn transition_to_state_record(&self) {
        transition_active_slice(self.transition_options(Stage::StateRecord, false))
            .expect("state-record transition should succeed");
    }

    fn finalize_options(&self, accept_broken_build: bool) -> FinalizeSliceOptions {
        FinalizeSliceOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            dispatch_log_path: self.root.join(".mutagen/state/dispatch-log.jsonl"),
            summary_root: self.root.join("slices"),
            slice_id: SLICE_ID.to_string(),
            completed_at: "2026-05-13T12:00:00Z".to_string(),
            accept_missing_artifacts: false,
            accept_broken_build,
        }
    }

    fn transition_options(
        &self,
        stage: Stage,
        bump_attempts: bool,
    ) -> TransitionActiveSliceOptions {
        TransitionActiveSliceOptions {
            queue_path: self.root.join("slices/queue.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            slice_id: SLICE_ID.to_string(),
            stage,
            active_agent: None,
            bump_attempts,
            bump_micro_corrections: false,
        }
    }

    fn write_author_output(&self, slice_id: &str, body: &str) {
        self.write_text(&format!(".mutagen/state/author-output/{slice_id}.md"), body);
    }

    fn write_text(&self, relative_path: &str, body: &str) {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture write parent should exist");
        }
        fs::write(path, body).expect("fixture text should write");
    }

    fn read_json(&self, relative_path: &str) -> Value {
        let raw =
            fs::read_to_string(self.root.join(relative_path)).expect("fixture JSON should read");
        serde_json::from_str(&raw).expect("fixture JSON should parse")
    }
}

impl Drop for FixtureWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();

    for attempt in 0..1024 {
        let path = env::temp_dir().join(format!(
            "mutagen-harness-{name}-finalize-buildable-{}-{nanos}-{attempt}",
            std::process::id()
        ));

        if fs::create_dir(&path).is_ok() {
            return path;
        }
    }

    panic!("failed to allocate a unique temp dir for {name}");
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(source_path, destination_path)?;
        }
    }

    Ok(())
}
