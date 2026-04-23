use mutagen_harness::adapter::HostKind;
use mutagen_harness::notifications::{NotificationKind, StopCondition};
use mutagen_harness::queue::{BishopVerdict, SliceStatus, TigerClawVerdict};
use mutagen_harness::queue_update::{UpdateSliceOptions, update_slice};
use mutagen_harness::review::{ReviewDecisionOptions, review_decision};
use mutagen_harness::runtime::{PrepareNextOptions, prepare_next};
use mutagen_harness::state::Stage;
use mutagen_harness::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn review_decision_continues_on_clean_and_marks_micro_correction_telemetry() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_review_ready_slice();
    workspace.set_review_verdict(TigerClawVerdict::Clean, None);
    workspace.write_review_reports(clean_report());
    workspace.set_micro_corrections_used(1);

    let result = review_decision(workspace.review_options("L1-orders-001"))
        .expect("review-decision should succeed");

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(value["action"], "continue");
    assert_eq!(value["tiger_claw"], "clean");
    assert_eq!(value["micro_correction_applied"], true);

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "in_progress");
    assert_eq!(queue["slices"][0]["verdicts"]["micro_correction"], true);
}

#[test]
fn review_decision_selects_micro_correction_for_machine_readable_retry_contract() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_review_ready_slice();
    workspace.set_review_verdict(TigerClawVerdict::Defect, None);
    workspace.write_review_reports(defect_report(
        true,
        "mechanical",
        &["tests/orders/aggregate_test.rs"],
        "Update the aggregate test wiring and keep the fix inside the existing order test file.",
    ));

    let result = review_decision(workspace.review_options("L1-orders-001"))
        .expect("review-decision should succeed");

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(value["action"], "micro_correction");
    assert_eq!(value["active_agent"], "Bebop");
    assert_eq!(
        value["suggested_fix_files"],
        json!(["tests/orders/aggregate_test.rs"])
    );

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "in_progress");
}

#[test]
fn review_decision_uses_bebop_fallback_for_legacy_scope_misses() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][0]["author_agent"] = json!("Krang");
    queue["slices"][0]["write_set"] = json!([]);
    workspace.write_json("slices/queue.json", &queue);

    workspace.prepare_review_ready_slice();
    workspace.set_review_verdict(TigerClawVerdict::Defect, None);
    workspace.write_review_reports(defect_report(
        true,
        "mechanical",
        &["tests/orders/aggregate_test.rs"],
        "Patch the QA miss in the order test and leave the infra slice alone.",
    ));

    let result = review_decision(workspace.review_options("L1-orders-001"))
        .expect("review-decision should succeed");

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(value["action"], "micro_correction");
    assert_eq!(value["active_agent"], "Bebop");
}

#[test]
fn review_decision_marks_blocked_retry_when_hatch_is_unavailable() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_review_ready_slice();
    workspace.set_review_verdict(TigerClawVerdict::Defect, None);
    workspace.write_review_reports(defect_report(
        false,
        "none",
        &[],
        "This defect needs a full author retry.",
    ));

    let result = review_decision(workspace.review_options("L1-orders-001"))
        .expect("review-decision should succeed");

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(value["action"], "retry");
    assert_eq!(value["status"], "blocked_retry");
    assert_eq!(value["reason"], "retry_contract_not_hatch_eligible");

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "blocked_retry");
}

#[test]
fn review_decision_escalates_when_retry_budget_is_exhausted() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_review_ready_slice();
    workspace.set_review_verdict(TigerClawVerdict::Defect, Some(3));
    workspace.write_review_reports(defect_report(
        false,
        "none",
        &[],
        "This defect exhausted the retry budget.",
    ));

    let result = review_decision(workspace.review_options("L1-orders-001"))
        .expect("review-decision should succeed");

    let value = serde_json::to_value(&result).expect("result should serialize");
    assert_eq!(value["action"], "escalated");
    assert_eq!(value["status"], "escalated");
    assert_eq!(
        value["escalation_reason"],
        "Tiger Claw Defect after 3 attempts (micro_corrections_used: 0)"
    );
    assert_eq!(value["stop_condition"], "retry_budget_exhausted");
    assert_eq!(value["notifications"][0]["event"], "escalation");

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "escalated");
    assert_eq!(
        queue["slices"][0]["escalation_reason"],
        "Tiger Claw Defect after 3 attempts (micro_corrections_used: 0)"
    );

    match result {
        mutagen_harness::review::ReviewDecisionResult::Escalated {
            stop_condition,
            notifications,
            ..
        } => {
            assert_eq!(stop_condition, StopCondition::RetryBudgetExhausted);
            assert_eq!(notifications[0].event, NotificationKind::Escalation);
        }
        other => panic!("expected escalated result, got {other:?}"),
    }
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

    fn prepare_review_ready_slice(&self) {
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

    fn set_review_verdict(&self, tiger_claw: TigerClawVerdict, attempts_override: Option<u32>) {
        update_slice(UpdateSliceOptions {
            queue_path: self.root.join("slices/queue.json"),
            slice_id: "L1-orders-001".to_string(),
            status: Some(SliceStatus::InProgress),
            attempts: attempts_override,
            micro_corrections_used: None,
            karai_structural: None,
            bishop: Some(BishopVerdict::Skip),
            tiger_claw: Some(tiger_claw),
            micro_correction: None,
            completed_at: None,
            clear_completed_at: false,
            escalation_reason: None,
            clear_escalation_reason: false,
        })
        .expect("queue update should succeed");

        if let Some(attempts) = attempts_override {
            let mut active_state = self.read_json(".mutagen/state/active-slice.json");
            active_state["attempts"] = json!(attempts);
            self.write_json(".mutagen/state/active-slice.json", &active_state);
        }
    }

    fn set_micro_corrections_used(&self, count: u32) {
        let mut active_state = self.read_json(".mutagen/state/active-slice.json");
        active_state["micro_corrections_used"] = json!(count);
        self.write_json(".mutagen/state/active-slice.json", &active_state);

        let mut queue = self.read_json("slices/queue.json");
        queue["slices"][0]["micro_corrections_used"] = json!(count);
        queue["slices"][0]["verdicts"]["micro_corrections_used"] = json!(count);
        self.write_json("slices/queue.json", &queue);
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

    fn write_json(&self, relative_path: &str, value: &Value) {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture write parent should exist");
        }

        let body = serde_json::to_string_pretty(value).expect("fixture JSON should serialize");
        fs::write(path, format!("{body}\n")).expect("fixture JSON should write");
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

fn defect_report(
    hatch_eligible: bool,
    suggested_fix_scope: &str,
    suggested_fix_files: &[&str],
    suggested_fix_summary: &str,
) -> String {
    let files = suggested_fix_files
        .iter()
        .map(|path| format!("\"{path}\""))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        r#"### 🐅 QA: L1-orders-001

#### Verdict
**🔴 Defect confirmed**

#### Retry Contract
```json
{{
  "hatch_eligible": {hatch_eligible},
  "suggested_fix_scope": "{suggested_fix_scope}",
  "suggested_fix_files": [{files}],
  "suggested_fix_summary": "{suggested_fix_summary}"
}}
```
"#
    )
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();

    env::temp_dir().join(format!(
        "mutagen-harness-{name}-review-decision-{}-{nanos}",
        std::process::id()
    ))
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
