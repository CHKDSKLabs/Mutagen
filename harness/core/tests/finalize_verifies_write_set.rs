// L1-Harness-002: ghost-completion regression suite. The 2026-05-05
// L1-Infra-003 incident finalized cleanly with zero artifacts actually on
// disk; these tests pin the new behavior — finalize walks the write_set,
// refuses to advance to `completed` when paths are missing, and audits the
// bypass when the operator types --accept-missing-artifacts.

use mutagen_core::adapter::HostKind;
use mutagen_core::finalize::{FinalizeSliceOptions, finalize_slice};
use mutagen_core::queue::{BishopVerdict, KaraiStructuralVerdict, SliceStatus, TigerClawVerdict};
use mutagen_core::queue_update::{UpdateSliceOptions, update_slice};
use mutagen_core::runtime::{PrepareNextOptions, prepare_next};
use mutagen_core::state::Stage;
use mutagen_core::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

const SLICE_ID: &str = "L1-orders-001";

#[test]
fn finalize_succeeds_when_write_set_artifacts_exist() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_claimed_slice();
    workspace.seed_author_artifacts();
    workspace.write_author_output(SLICE_ID, &author_output_body());
    workspace.write_text("project_state.md", "# Project State\n");
    workspace.set_clean_verdicts();
    workspace.transition_to_state_record();

    let result = finalize_slice(workspace.finalize_options(false))
        .expect("finalize should succeed when artifacts exist on disk");

    assert_eq!(result.status, SliceStatus::Completed);
    assert!(result.missing_artifacts.is_empty());
    assert!(!result.finalize_artifacts_overridden);

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "completed");

    assert!(
        !workspace
            .root
            .join(".mutagen/state/finalize-audit.jsonl")
            .exists(),
        "no audit record should land on the clean path"
    );
}

#[test]
fn missing_artifacts_blocks_finalize() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_claimed_slice();
    // The whole point: agent claims to have written files, but nothing lands.
    workspace.write_author_output(SLICE_ID, &author_output_body());
    workspace.write_text("project_state.md", "# Project State\n");
    workspace.set_clean_verdicts();
    workspace.transition_to_state_record();

    let error = finalize_slice(workspace.finalize_options(false))
        .expect_err("finalize should refuse when write_set artifacts are missing");

    let message = error.to_string();
    assert!(
        message.contains("write_set paths missing"),
        "unexpected error: {message}"
    );
    assert!(
        message.contains("src/orders/**"),
        "missing path not reported: {message}"
    );
    assert!(
        message.contains("tests/orders/**"),
        "missing path not reported: {message}"
    );

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "finalization_failed");
    assert!(
        queue["slices"][0]["escalation_reason"]
            .as_str()
            .unwrap_or_default()
            .contains("write_set_artifacts_missing"),
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
        "non-override path should not emit an audit record"
    );
}

#[test]
fn blocked_finalize_does_not_mutate_project_state_md() {
    // Tiger Claw L1-Harness-002 round 1 reproducer: when the write_set gate
    // blocked, apply_and_verify_state_update had already written the slice's
    // State Update section into project_state.md. Queue flipped to
    // finalization_failed but the context file lied about a clean finalize.
    // The transition must be atomic — gate fails closed with zero side
    // effects on the context file.
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_claimed_slice();
    workspace.write_author_output(SLICE_ID, &author_output_body());
    workspace.write_text("project_state.md", "# Project State\n");
    workspace.set_clean_verdicts();
    workspace.transition_to_state_record();

    let context_path = workspace.root.join("project_state.md");
    let before = fs::read_to_string(&context_path).expect("project_state.md should read");

    let _ = finalize_slice(workspace.finalize_options(false))
        .expect_err("blocked finalize should bail before applying state update");

    let after = fs::read_to_string(&context_path).expect("project_state.md should read");
    assert_eq!(
        before, after,
        "blocked finalize must not leak state-update side-effects into project_state.md"
    );
}

#[test]
fn accept_missing_artifacts_records_override() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_claimed_slice();
    workspace.write_author_output(SLICE_ID, &author_output_body());
    workspace.write_text("project_state.md", "# Project State\n");
    workspace.set_clean_verdicts();
    workspace.transition_to_state_record();

    let result = finalize_slice(workspace.finalize_options(true))
        .expect("override should finalize even when artifacts are missing");

    assert_eq!(result.status, SliceStatus::Completed);
    assert!(result.finalize_artifacts_overridden);
    assert!(!result.missing_artifacts.is_empty());

    let audit_path = workspace.root.join(".mutagen/state/finalize-audit.jsonl");
    let audit_body = fs::read_to_string(&audit_path).expect("override should leave audit record");
    let first_line = audit_body
        .lines()
        .next()
        .expect("audit log must have a row");
    let entry: Value = serde_json::from_str(first_line).expect("audit row should parse");
    assert_eq!(entry["event"], "slice.finalize_artifacts_overridden");
    assert_eq!(entry["slice_id"], SLICE_ID);
    assert!(
        entry["missing"]
            .as_array()
            .map(|arr| !arr.is_empty())
            .unwrap_or(false),
        "audit record should carry the missing-paths list"
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

    fn finalize_options(&self, accept_missing_artifacts: bool) -> FinalizeSliceOptions {
        FinalizeSliceOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            dispatch_log_path: self.root.join(".mutagen/state/dispatch-log.jsonl"),
            summary_root: self.root.join("slices"),
            slice_id: SLICE_ID.to_string(),
            completed_at: "2026-05-13T12:00:00Z".to_string(),
            accept_missing_artifacts,
            accept_broken_build: false,
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
            "mutagen-harness-{name}-finalize-write-set-{}-{nanos}-{attempt}",
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
