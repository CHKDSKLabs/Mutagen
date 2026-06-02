use mutagen_core::adapter::HostKind;
use mutagen_core::queue::SliceStatus;
use mutagen_core::resume_slice::{ResumeSliceOptions, resume_slice};
use mutagen_core::state::Stage;
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

#[test]
fn resume_slice_resets_active_state_to_review_stage() {
    let workspace = FixtureWorkspace::copy("basic_ready");

    // Pretend a previous wedged run left the active-slice file pointing at a different
    // slice with a stale stage. Resume should clobber it cleanly.
    workspace.write_text(
        ".mutagen/state/active-slice.json",
        r#"{
  "slice_id": "L1-zombie-999",
  "title": "wedged",
  "evidence_bundle_path": "stale.md",
  "author_agent": "Bebop",
  "active_agent": "Bebop",
  "stage": "author",
  "pipeline_mode": "full",
  "review_required": true,
  "layer": 1,
  "bounded_context": "stale",
  "context_to_update": "stale.md",
  "attempts": 7,
  "max_retries": 3,
  "micro_corrections_used": 0,
  "max_micro_corrections": 2,
  "allowed_write_globs": [],
  "host": "codex"
}
"#,
    );

    let result = resume_slice(ResumeSliceOptions {
        workspace_root: workspace.root.clone(),
        queue_path: workspace.root.join("slices/queue.json"),
        queue_validation_path: support::queue_validation_path(&workspace.root),
        workflow_config_path: workspace.root.join(".claude/workflow.json"),
        active_state_path: workspace.root.join(".mutagen/state/active-slice.json"),
        slice_id: "L1-orders-001".to_string(),
        from_stage: Stage::Review,
        host: HostKind::Codex,
    })
    .expect("resume should succeed");

    assert_eq!(result.slice_id, "L1-orders-001");
    assert_eq!(result.from_stage, Stage::Review);
    assert_eq!(
        result.previous_active_slice_id.as_deref(),
        Some("L1-zombie-999")
    );
    assert_eq!(result.previous_stage, Some(Stage::Author));
    assert_eq!(result.status, SliceStatus::InProgress);
    assert_eq!(result.active_agent, "TigerClaw");
    assert!(
        result
            .allowed_write_globs
            .iter()
            .any(|glob| glob == "reviews/**"),
        "review-stage scope must include reviews/**, got {:?}",
        result.allowed_write_globs
    );

    let active_state = workspace.read_json(".mutagen/state/active-slice.json");
    assert_eq!(active_state["slice_id"], "L1-orders-001");
    assert_eq!(active_state["stage"], "review");
    assert_eq!(active_state["active_agent"], "TigerClaw");

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "in_progress");
}

#[test]
fn resume_slice_refuses_terminal_status() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let queue_path = workspace.root.join("slices/queue.json");
    let mut queue: Value =
        serde_json::from_str(&fs::read_to_string(&queue_path).expect("queue should read"))
            .expect("queue should parse");
    queue["slices"][0]["status"] = Value::String("completed".to_string());
    fs::write(
        &queue_path,
        serde_json::to_string_pretty(&queue).expect("queue should serialize"),
    )
    .expect("queue should write");

    let err = resume_slice(ResumeSliceOptions {
        workspace_root: workspace.root.clone(),
        queue_path: queue_path.clone(),
        queue_validation_path: support::queue_validation_path(&workspace.root),
        workflow_config_path: workspace.root.join(".claude/workflow.json"),
        active_state_path: workspace.root.join(".mutagen/state/active-slice.json"),
        slice_id: "L1-orders-001".to_string(),
        from_stage: Stage::Author,
        host: HostKind::Codex,
    })
    .expect_err("resume on completed slice should refuse");

    let message = format!("{err:#}");
    assert!(message.contains("completed"), "got: {message}");
    assert!(message.contains("terminal"), "got: {message}");
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
            "mutagen-harness-{name}-resume-slice-{}-{nanos}-{attempt}",
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
