use mutagen_harness::adapter::HostKind;
use mutagen_harness::finalize::{FinalizeSliceOptions, finalize_slice};
use mutagen_harness::notifications::NotificationKind;
use mutagen_harness::queue::{
    BishopVerdict, KaraiStructuralVerdict, SliceStatus, TigerClawVerdict,
};
use mutagen_harness::queue_update::{UpdateSliceOptions, update_slice};
use mutagen_harness::runtime::{PrepareNextOptions, prepare_next};
use mutagen_harness::state::Stage;
use mutagen_harness::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn finalize_slice_writes_summary_dispatch_log_and_clears_active_state() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_claimed_slice();
    workspace.write_author_output(
        "L1-orders-001",
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
### L1-orders-001 — 2026-04-22
**Artifacts:** src/orders/aggregate.rs, tests/orders/aggregate_test.rs
"#,
    );
    workspace.write_text("project_state.md", "# Project State\n");
    workspace.write_text(
        "reviews/L1-orders-001/tiger-claw.md",
        "## Verdict\n🟢 Clean\n",
    );

    update_slice(UpdateSliceOptions {
        queue_path: workspace.root.join("slices/queue.json"),
        slice_id: "L1-orders-001".to_string(),
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
    })
    .expect("queue update should succeed");

    transition_active_slice(workspace.transition_options(
        "L1-orders-001",
        Stage::StateRecord,
        None,
        false,
        false,
    ))
    .expect("state-record transition should succeed");

    let result = finalize_slice(workspace.finalize_options("L1-orders-001"))
        .expect("finalize-slice should succeed");

    assert_eq!(result.status, SliceStatus::Completed);
    assert_eq!(result.completed_at, "2026-04-22T18:00:00Z");
    assert_eq!(result.retry_path, "first-pass clean");
    assert!(result.state_verified);
    assert!(result.duration_seconds.is_some());
    assert_eq!(
        result.files_touched,
        vec![
            "src/orders/aggregate.rs".to_string(),
            "tests/orders/aggregate_test.rs".to_string()
        ]
    );
    assert_eq!(
        result.completion_marker,
        "✔ L1-orders-001 — clean, attempts=1"
    );
    assert!(result.layer_complete);
    assert_eq!(result.completed_in_layer, 1);
    assert_eq!(
        result.next_pending_slice_id.as_deref(),
        Some("L2-orders-002")
    );
    assert_eq!(result.notifications.len(), 1);
    assert_eq!(
        result.notifications[0].event,
        NotificationKind::LayerComplete
    );

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "completed");
    assert_eq!(queue["slices"][0]["completed_at"], "2026-04-22T18:00:00Z");

    assert!(
        !workspace
            .root
            .join(".mutagen/state/active-slice.json")
            .exists(),
        "successful finalization should clear the active slice"
    );

    let summary = workspace.read_text("slices/L1-orders-001/summary.md");
    assert!(summary.contains("# Slice summary — L1-orders-001"));
    assert!(summary.contains("**Completed at:** 2026-04-22T18:00:00Z"));
    assert!(summary.contains("- `src/orders/aggregate.rs`"));
    assert!(summary.contains("- QA: `reviews/L1-orders-001/tiger-claw.md`"));
    assert!(summary.contains("- Evidence: `.mutagen/state/evidence/L1-orders-001.md`"));

    let project_state = workspace.read_text("project_state.md");
    assert!(project_state.contains("### L1-orders-001 — 2026-04-22"));
    assert!(
        project_state
            .contains("**Artifacts:** src/orders/aggregate.rs, tests/orders/aggregate_test.rs")
    );

    let dispatch_log = workspace.read_text(".mutagen/state/dispatch-log.jsonl");
    let first_line = dispatch_log
        .lines()
        .next()
        .expect("dispatch log should contain one line");
    let entry: Value = serde_json::from_str(first_line).expect("dispatch log line should parse");
    assert_eq!(entry["slice_id"], "L1-orders-001");
    assert_eq!(entry["status"], "completed");
    assert_eq!(entry["host"], "codex");
    assert_eq!(entry["state_verified"], true);
    assert_eq!(entry["summary_path"], "slices/L1-orders-001/summary.md");
}

#[test]
fn finalize_slice_refuses_when_state_update_block_is_missing_slice_marker() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_claimed_slice();
    workspace.write_author_output(
        "L1-orders-001",
        r#"### 🛠️ Execution: L1-orders-001
#### Intake Report
- Domain fit: standard execution ✓
#### Code Artifacts
- `src/orders/aggregate.rs`
#### ISC Upholding Map
| ISC | Code site | Mechanism | Detection test |
#### Verification Artifacts
- Acceptance: cargo test
#### State Update
Completed, probably. Details are elsewhere.
"#,
    );
    workspace.write_text("project_state.md", "# Project State\n");

    update_slice(UpdateSliceOptions {
        queue_path: workspace.root.join("slices/queue.json"),
        slice_id: "L1-orders-001".to_string(),
        status: None,
        attempts: None,
        micro_corrections_used: None,
        karai_structural: Some(KaraiStructuralVerdict::Pass),
        bishop: Some(BishopVerdict::Skip),
        tiger_claw: Some(TigerClawVerdict::Gap),
        micro_correction: Some(false),
        completed_at: None,
        clear_completed_at: false,
        escalation_reason: None,
        clear_escalation_reason: false,
    })
    .expect("queue update should succeed");

    transition_active_slice(workspace.transition_options(
        "L1-orders-001",
        Stage::StateRecord,
        None,
        false,
        false,
    ))
    .expect("state-record transition should succeed");

    let error = finalize_slice(workspace.finalize_options("L1-orders-001"))
        .expect_err("finalize-slice should fail when the state update is missing");

    assert!(
        error
            .to_string()
            .contains("State Update block must start with a slice marker"),
        "unexpected error: {error:#}"
    );

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "in_progress");
    assert!(
        workspace
            .root
            .join(".mutagen/state/active-slice.json")
            .exists(),
        "failed finalization should leave the active slice in place"
    );
    assert!(
        !workspace
            .root
            .join("slices/L1-orders-001/summary.md")
            .exists(),
        "failed finalization should not create a summary"
    );
    assert!(
        !workspace
            .root
            .join(".mutagen/state/dispatch-log.jsonl")
            .exists(),
        "failed finalization should not append a dispatch log entry"
    );
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

        Self { root: destination }
    }

    fn prepare_claimed_slice(&self) {
        prepare_next(PrepareNextOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            workflow_config_path: self.root.join(".claude/workflow.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            host: HostKind::Codex,
            dry_run: false,
        })
        .expect("prepare-next should succeed");

        transition_active_slice(self.transition_options(
            "L1-orders-001",
            Stage::Author,
            None,
            true,
            false,
        ))
        .expect("author transition should succeed");
    }

    fn finalize_options(&self, slice_id: &str) -> FinalizeSliceOptions {
        FinalizeSliceOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            dispatch_log_path: self.root.join(".mutagen/state/dispatch-log.jsonl"),
            summary_root: self.root.join("slices"),
            slice_id: slice_id.to_string(),
            completed_at: "2026-04-22T18:00:00Z".to_string(),
        }
    }

    fn transition_options(
        &self,
        slice_id: &str,
        stage: Stage,
        active_agent: Option<String>,
        bump_attempts: bool,
        bump_micro_corrections: bool,
    ) -> TransitionActiveSliceOptions {
        TransitionActiveSliceOptions {
            queue_path: self.root.join("slices/queue.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            slice_id: slice_id.to_string(),
            stage,
            active_agent,
            bump_attempts,
            bump_micro_corrections,
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

    fn read_text(&self, relative_path: &str) -> String {
        fs::read_to_string(self.root.join(relative_path)).expect("fixture text should read")
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
            "mutagen-harness-{name}-finalize-slice-{}-{nanos}-{attempt}",
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
