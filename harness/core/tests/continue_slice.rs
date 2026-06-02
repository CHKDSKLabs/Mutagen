use mutagen_core::adapter::HostKind;
use mutagen_core::runner::{ContinueSliceOptions, run_continue_slice};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

#[test]
fn continue_slice_refuses_when_no_active_slice() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let outcome = run_continue_slice(continue_options(&workspace, None));
    let payload = outcome.payload;

    assert_eq!(payload["ok"], false, "payload: {payload}");
    assert_eq!(payload["error"], "no_active_slice", "payload: {payload}");
    assert_eq!(outcome.exit_code, 1, "payload: {payload}");
}

#[test]
fn continue_slice_refuses_when_slice_id_mismatches() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_active_slice("L1-orders-001", "structural_check");

    let mut options = continue_options(&workspace, None);
    options.slice_id = Some("L2-orders-002".to_string());

    let outcome = run_continue_slice(options);
    let payload = outcome.payload;

    assert_eq!(payload["ok"], false, "payload: {payload}");
    assert_eq!(
        payload["error"], "active_slice_mismatch",
        "payload: {payload}"
    );
    assert_eq!(payload["slice_id"], "L1-orders-001", "payload: {payload}");
}

#[test]
fn continue_slice_refuses_when_stage_is_author() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_active_slice("L1-orders-001", "author");

    let outcome = run_continue_slice(continue_options(&workspace, None));
    let payload = outcome.payload;

    assert_eq!(payload["ok"], false, "payload: {payload}");
    assert_eq!(
        payload["error"], "not_resumable_at_author",
        "payload: {payload}"
    );
}

#[test]
fn continue_slice_refuses_when_slice_already_completed() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_active_slice("L1-orders-001", "state_record");
    workspace.set_slice_status("L1-orders-001", "completed");

    let outcome = run_continue_slice(continue_options(&workspace, None));
    let payload = outcome.payload;

    assert_eq!(payload["ok"], false, "payload: {payload}");
    assert_eq!(
        payload["error"], "slice_already_terminal",
        "payload: {payload}"
    );
}

fn continue_options(workspace: &FixtureWorkspace, slice_id: Option<&str>) -> ContinueSliceOptions {
    ContinueSliceOptions {
        workspace_root: workspace.root.clone(),
        queue_path: workspace.root.join("slices/queue.json"),
        queue_validation_path: support::queue_validation_path(&workspace.root),
        workflow_config_path: workspace.root.join(".claude/workflow.json"),
        active_state_path: workspace.root.join(".mutagen/state/active-slice.json"),
        author_output_dir: workspace.root.join(".mutagen/state/author-output"),
        dispatch_root: workspace.root.join(".mutagen/state/dispatch"),
        dispatch_log_path: workspace.root.join(".mutagen/state/dispatch-log.jsonl"),
        summary_root: workspace.root.join("slices"),
        slicemap_path: workspace.root.join("slices/slicemap.md"),
        legacy_path: workspace.root.join("slices/queue.md"),
        host: HostKind::Stub,
        slice_id: slice_id.map(str::to_string),
        mutagen_root: None,
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

        let workspace = Self { root: destination };
        support::write_queue_validation(&workspace.root);
        workspace
    }

    fn write_active_slice(&self, slice_id: &str, stage: &str) {
        let body = format!(
            r#"{{
  "slice_id": "{slice_id}",
  "title": "Create order aggregate",
  "evidence_bundle_path": ".mutagen/state/evidence/{slice_id}.md",
  "author_agent": "Bebop",
  "active_agent": "Bebop",
  "stage": "{stage}",
  "pipeline_mode": "full",
  "review_required": true,
  "layer": 1,
  "bounded_context": "orders",
  "context_to_update": "project_state.md",
  "attempts": 1,
  "max_retries": 3,
  "micro_corrections_used": 0,
  "max_micro_corrections": 2,
  "allowed_write_globs": [],
  "host": "stub"
}}
"#,
        );
        let path = self.root.join(".mutagen/state/active-slice.json");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("active-slice parent should exist");
        }
        fs::write(path, body).expect("active-slice write");
    }

    fn set_slice_status(&self, slice_id: &str, status: &str) {
        let queue_path = self.root.join("slices/queue.json");
        let mut queue: Value =
            serde_json::from_str(&fs::read_to_string(&queue_path).expect("queue should read"))
                .expect("queue should parse");
        let slices = queue["slices"].as_array_mut().expect("slices array");
        for slice in slices {
            if slice["id"].as_str() == Some(slice_id) {
                slice["status"] = Value::String(status.to_string());
            }
        }
        fs::write(
            &queue_path,
            serde_json::to_string_pretty(&queue).expect("queue should serialize"),
        )
        .expect("queue should write");
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
            "mutagen-harness-{name}-continue-slice-{}-{nanos}-{attempt}",
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
