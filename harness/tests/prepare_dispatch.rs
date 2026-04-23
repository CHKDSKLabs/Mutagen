use mutagen_harness::adapter::HostKind;
use mutagen_harness::dispatch::{AuthorDispatchKind, PrepareDispatchOptions, prepare_dispatch};
use mutagen_harness::runtime::{PrepareNextOptions, prepare_next};
use mutagen_harness::state::Stage;
use mutagen_harness::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn prepare_dispatch_writes_initial_author_prompt() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_ready_slice();

    let result = prepare_dispatch(workspace.dispatch_options(None))
        .expect("author dispatch preparation should succeed");

    assert_eq!(result.stage, Stage::Author);
    assert_eq!(result.agent, "Bebop");
    assert_eq!(result.dispatch_kind, Some(AuthorDispatchKind::Initial));
    assert!(result.required_written_artifacts.is_empty());

    let prompt = fs::read_to_string(&result.prompt_path).expect("prompt should read");
    assert!(prompt.contains("# Author Dispatch"));
    assert!(prompt.contains("Dispatch kind: initial"));
    assert!(prompt.contains("Read this bundle once before coding"));
    assert!(prompt.contains("Allowed write globs"));
}

#[test]
fn prepare_dispatch_writes_retry_author_prompt_when_qa_report_exists() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_ready_slice();
    workspace.write_text(
        "reviews/L1-orders-001/tiger-claw.md",
        "#### Suggested Fix\n- Tighten the boundary check.\n",
    );

    let result = prepare_dispatch(workspace.dispatch_options(Some(AuthorDispatchKind::Retry)))
        .expect("retry author dispatch preparation should succeed");

    assert_eq!(result.dispatch_kind, Some(AuthorDispatchKind::Retry));
    assert!(result.qa_report_path.is_some());

    let prompt = fs::read_to_string(&result.prompt_path).expect("prompt should read");
    assert!(prompt.contains("Dispatch kind: retry"));
    assert!(prompt.contains("Prior Tiger Claw QA report"));
    assert!(prompt.contains("Address every Suggested Fix"));
}

#[test]
fn prepare_dispatch_writes_review_prompt_and_expected_report_paths() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_ready_slice();
    workspace.write_text(
        ".mutagen/state/author-output/L1-orders-001.md",
        "### 🛠️ Execution: L1-orders-001\n",
    );
    workspace.transition_to_review();

    let result = prepare_dispatch(workspace.dispatch_options(None))
        .expect("review dispatch preparation should succeed");

    assert_eq!(result.stage, Stage::Review);
    assert_eq!(result.agent, "TigerClaw");
    assert_eq!(result.required_written_artifacts.len(), 2);

    let prompt = fs::read_to_string(&result.prompt_path).expect("prompt should read");
    assert!(prompt.contains("# Review Dispatch"));
    assert!(prompt.contains("Author output to review"));
    assert!(prompt.contains("Write the QA report to"));
    assert!(prompt.contains("Retry Contract block is mandatory"));
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

    fn prepare_ready_slice(&self) {
        prepare_next(PrepareNextOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            workflow_config_path: self.root.join(".claude/workflow.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            host: HostKind::Codex,
            dry_run: false,
        })
        .expect("prepare-next should succeed");
    }

    fn transition_to_review(&self) {
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

    fn dispatch_options(
        &self,
        dispatch_kind: Option<AuthorDispatchKind>,
    ) -> PrepareDispatchOptions {
        PrepareDispatchOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            author_output_dir: self.root.join(".mutagen/state/author-output"),
            dispatch_root: self.root.join(".mutagen/state/dispatch"),
            qa_report_path: None,
            latest_qa_report_path: None,
            slice_id: "L1-orders-001".to_string(),
            dispatch_kind,
        }
    }

    fn write_text(&self, relative_path: &str, body: &str) {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should exist");
        }

        fs::write(path, body).expect("fixture text should write");
    }

    #[allow(dead_code)]
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
            "mutagen-harness-{name}-prepare-dispatch-{}-{nanos}-{attempt}",
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
