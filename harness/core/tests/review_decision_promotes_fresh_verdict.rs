// L1-Harness-004: review-decision must take the freshly-parsed QA verdict as
// gospel and overwrite a stale queue snapshot. The 2026-05-12 L4-Workflow-001
// incident is the shape this guards against — retry produced `clean`, queue
// kept `defect`, slice stayed escalated until somebody hand-typed an update.

use mutagen_core::adapter::HostKind;
use mutagen_core::queue::{BishopVerdict, SliceStatus, TigerClawVerdict};
use mutagen_core::queue_update::{UpdateSliceOptions, update_slice};
use mutagen_core::review::{ReviewDecisionOptions, review_decision};
use mutagen_core::runtime::{PrepareNextOptions, prepare_next};
use mutagen_core::state::Stage;
use mutagen_core::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

#[test]
fn clean_report_overrides_stale_defect() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_review_ready_slice();
    workspace.stamp_stale_defect();
    workspace.write_review_reports(clean_report());

    let queue_before = workspace.read_json("slices/queue.json");
    assert_eq!(
        queue_before["slices"][0]["verdicts"]["tiger_claw"], "defect",
        "fixture should stage a stale defect before the fresh review runs"
    );

    let result = review_decision(workspace.review_options("L1-orders-001"))
        .expect("review-decision should succeed");

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(value["action"], "continue");
    assert_eq!(value["tiger_claw"], "clean");

    let queue_after = workspace.read_json("slices/queue.json");
    assert_eq!(
        queue_after["slices"][0]["verdicts"]["tiger_claw"], "clean",
        "fresh clean verdict must overwrite the stale defect on disk"
    );
    assert_eq!(queue_after["slices"][0]["status"], "in_progress");

    let result_again = review_decision(workspace.review_options("L1-orders-001"))
        .expect("re-running review-decision on the same fresh report should succeed");
    let value_again = serde_json::to_value(&result_again).expect("result should serialize");
    assert_eq!(
        value_again["action"], "continue",
        "idempotent re-run must return the same action"
    );
    assert_eq!(value_again["tiger_claw"], "clean");

    let queue_final = workspace.read_json("slices/queue.json");
    assert_eq!(
        queue_final["slices"][0]["verdicts"]["tiger_claw"], "clean",
        "idempotent re-run must leave the verdict at clean"
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

        let workspace = Self { root: destination };
        support::write_queue_validation(&workspace.root);
        workspace
    }

    fn prepare_review_ready_slice(&self) {
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

        transition_active_slice(self.transition_options(
            "L1-orders-001",
            Stage::Author,
            None,
            true,
            false,
        ))
        .expect("author transition should succeed");

        transition_active_slice(self.transition_options(
            "L1-orders-001",
            Stage::Review,
            None,
            false,
            false,
        ))
        .expect("review transition should succeed");
    }

    fn review_options(&self, slice_id: &str) -> ReviewDecisionOptions {
        ReviewDecisionOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            qa_report_path: Some(
                self.root
                    .join("reviews")
                    .join(slice_id)
                    .join("tiger-claw.md"),
            ),
            latest_qa_report_path: Some(self.root.join(".mutagen/state/tiger-claw-latest.md")),
            slice_id: slice_id.to_string(),
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

    fn stamp_stale_defect(&self) {
        update_slice(UpdateSliceOptions {
            queue_path: self.root.join("slices/queue.json"),
            slice_id: "L1-orders-001".to_string(),
            status: Some(SliceStatus::InProgress),
            attempts: None,
            micro_corrections_used: None,
            karai_structural: None,
            bishop: Some(BishopVerdict::Skip),
            tiger_claw: Some(TigerClawVerdict::Defect),
            micro_correction: None,
            completed_at: None,
            clear_completed_at: false,
            escalation_reason: None,
            clear_escalation_reason: false,
            human_check_resolved_at: None,
            clear_human_check_resolved_at: false,
        })
        .expect("queue update should succeed");
    }

    fn write_review_reports(&self, body: String) {
        self.write_text("reviews/L1-orders-001/tiger-claw.md", &body);
        self.write_text(".mutagen/state/tiger-claw-latest.md", &body);
    }

    fn read_json(&self, relative_path: &str) -> Value {
        let raw =
            fs::read_to_string(self.root.join(relative_path)).expect("fixture JSON should read");
        serde_json::from_str(&raw).expect("fixture JSON should parse")
    }

    fn write_text(&self, relative_path: &str, body: &str) {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture write parent should exist");
        }

        fs::write(path, body).expect("fixture text should write");
    }
}

impl Drop for FixtureWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn clean_report() -> String {
    r#"### 🐅 QA: L1-orders-001

#### Verdict
**🟢 Clean**

#### Retry Contract
```json
{
  "hatch_eligible": false,
  "suggested_fix_scope": "none",
  "suggested_fix_files": [],
  "suggested_fix_summary": ""
}
```
"#
    .to_string()
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();

    for attempt in 0..1024 {
        let path = env::temp_dir().join(format!(
            "mutagen-harness-{name}-fresh-verdict-{}-{nanos}-{attempt}",
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
