use mutagen_harness::adapter::HostKind;
use mutagen_harness::queue::TigerClawVerdict;
use mutagen_harness::review_record::{RecordReviewVerdictOptions, record_review_verdict};
use mutagen_harness::runtime::{PrepareNextOptions, prepare_next};
use mutagen_harness::state::Stage;
use mutagen_harness::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn record_review_verdict_records_clean_and_bishop_skip() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_review_ready_slice();
    workspace.write_review_reports(clean_report(), clean_report());

    let result = record_review_verdict(workspace.review_options("L1-orders-001"))
        .expect("review verdict recording should succeed");

    assert_eq!(result.bishop, mutagen_harness::queue::BishopVerdict::Skip);
    assert_eq!(result.tiger_claw, TigerClawVerdict::Clean);

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["verdicts"]["bishop"], "skip");
    assert_eq!(queue["slices"][0]["verdicts"]["tiger_claw"], "clean");
    assert_eq!(queue["slices"][0]["status"], "in_progress");
}

#[test]
fn record_review_verdict_records_defect() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_review_ready_slice();
    workspace.write_review_reports(defect_report(), defect_report());

    let result = record_review_verdict(workspace.review_options("L1-orders-001"))
        .expect("review verdict recording should succeed");

    assert_eq!(result.tiger_claw, TigerClawVerdict::Defect);

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["verdicts"]["tiger_claw"], "defect");
}

#[test]
fn record_review_verdict_rejects_mismatched_latest_copy() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_review_ready_slice();
    workspace.write_review_reports(clean_report(), defect_report());

    let error = record_review_verdict(workspace.review_options("L1-orders-001"))
        .expect_err("review verdict recording should fail on mismatched copies");

    assert!(
        error.to_string().contains("Tiger Claw verdict mismatch"),
        "unexpected error: {error:?}"
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

        transition_active_slice(TransitionActiveSliceOptions {
            queue_path: self.root.join("slices/queue.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            slice_id: "L1-orders-001".to_string(),
            stage: Stage::Review,
            active_agent: None,
            bump_attempts: false,
            bump_micro_corrections: false,
        })
        .expect("review transition should succeed");
    }

    fn review_options(&self, slice_id: &str) -> RecordReviewVerdictOptions {
        RecordReviewVerdictOptions {
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

    fn read_json(&self, relative_path: &str) -> Value {
        let raw =
            fs::read_to_string(self.root.join(relative_path)).expect("fixture JSON should read");
        serde_json::from_str(&raw).expect("fixture JSON should parse")
    }

    fn write_review_reports(&self, qa_report: String, latest_report: String) {
        self.write_text("reviews/L1-orders-001/tiger-claw.md", &qa_report);
        self.write_text(".mutagen/state/tiger-claw-latest.md", &latest_report);
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

fn defect_report() -> String {
    r#"### 🐅 QA: L1-orders-001

#### Verdict
**🔴 Defect confirmed**

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
            "mutagen-harness-{name}-review-record-{}-{nanos}-{attempt}",
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
