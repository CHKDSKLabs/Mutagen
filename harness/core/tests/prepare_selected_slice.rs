use mutagen_core::adapter::{HostKind, ParallelDispatchMode, ScopeEnforcementMode};
use mutagen_core::queue::SliceQueue;
use mutagen_core::selected_slice::{
    PrepareSelectedSliceOptions, PrepareSelectedSliceResult, SelectedSliceBlockReason,
    prepare_selected_slice,
};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

#[test]
fn prepare_selected_slice_claims_requested_slice_and_writes_state() {
    let workspace = FixtureWorkspace::copy("basic_ready");

    let result = prepare_selected_slice(workspace.prepare_selected_options("L1-orders-001", false))
        .expect("prepare-selected-slice should succeed");

    let evidence_bundle_path = match result {
        PrepareSelectedSliceResult::Ready { prepared } => {
            assert_eq!(prepared.slice_id, "L1-orders-001");
            assert_eq!(prepared.author_agent, "Bebop");
            assert!(prepared.claimed);
            assert!(
                prepared
                    .degraded_capabilities
                    .contains(&"parallel_dispatch".to_string())
            );
            assert_eq!(
                prepared.host_profile.scope_enforcement,
                ScopeEnforcementMode::Advisory
            );
            assert_eq!(
                prepared.host_profile.parallel_dispatch,
                ParallelDispatchMode::SerialOnly
            );
            prepared.evidence_bundle_path
        }
        other => panic!("expected ready result, got {other:?}"),
    };

    let active_state = workspace.read_json(".mutagen/state/active-slice.json");
    assert_eq!(active_state["slice_id"], "L1-orders-001");
    assert_eq!(active_state["evidence_bundle_path"], evidence_bundle_path);

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "in_progress");
}

#[test]
fn prepare_selected_slice_returns_blocked_when_dependencies_are_unmet() {
    let workspace = FixtureWorkspace::copy("basic_ready");

    let result = prepare_selected_slice(workspace.prepare_selected_options("L2-orders-002", true))
        .expect("prepare-selected-slice should return blocked result");

    match result {
        PrepareSelectedSliceResult::Blocked {
            slice_id,
            reason,
            current_status,
            unmet_dependencies,
        } => {
            assert_eq!(slice_id, "L2-orders-002");
            assert_eq!(reason, SelectedSliceBlockReason::UnmetDependencies);
            assert_eq!(current_status, mutagen_core::queue::SliceStatus::Pending);
            assert_eq!(unmet_dependencies, vec!["L1-orders-001".to_string()]);
        }
        other => panic!("expected blocked result, got {other:?}"),
    }
}

#[test]
fn prepare_selected_slice_blocks_unresolved_human_check() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][0]["human_check_needed"] = json!({
        "required": true,
        "reason": "Project owner needs to pick the API shape.",
        "resolved_at": null
    });
    workspace.write_json("slices/queue.json", &queue);
    support::write_queue_validation(&workspace.root);

    let result = prepare_selected_slice(workspace.prepare_selected_options("L1-orders-001", false))
        .expect("prepare-selected-slice should return blocked result");

    match result {
        PrepareSelectedSliceResult::Blocked {
            slice_id,
            reason,
            current_status,
            unmet_dependencies,
        } => {
            assert_eq!(slice_id, "L1-orders-001");
            assert_eq!(reason, SelectedSliceBlockReason::PendingHumanCheck);
            assert_eq!(current_status, mutagen_core::queue::SliceStatus::Pending);
            assert!(unmet_dependencies.is_empty());
        }
        other => panic!("expected blocked result, got {other:?}"),
    }
}

#[test]
fn prepare_selected_slice_allows_existing_in_progress_slice_without_reclaiming() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][0]["status"] = json!("in_progress");
    workspace.write_json("slices/queue.json", &queue);

    let result = prepare_selected_slice(workspace.prepare_selected_options("L1-orders-001", false))
        .expect("prepare-selected-slice should succeed for in-progress slice");

    match result {
        PrepareSelectedSliceResult::Ready { prepared } => {
            assert_eq!(prepared.slice_id, "L1-orders-001");
            assert!(!prepared.claimed);
        }
        other => panic!("expected ready result, got {other:?}"),
    }

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "in_progress");
    assert!(
        workspace
            .root
            .join(".mutagen/state/active-slice.json")
            .exists()
    );
}

#[test]
fn validate_queue_fixture_still_deserializes_after_selected_prep_changes() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let queue: SliceQueue = serde_json::from_value(workspace.read_json("slices/queue.json"))
        .expect("fixture queue should deserialize");

    assert_eq!(queue.slices.len(), 2);
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

    fn prepare_selected_options(
        &self,
        slice_id: &str,
        dry_run: bool,
    ) -> PrepareSelectedSliceOptions {
        PrepareSelectedSliceOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            queue_validation_path: support::queue_validation_path(&self.root),
            workflow_config_path: self.root.join(".claude/workflow.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            slice_id: slice_id.to_string(),
            host: HostKind::Codex,
            dry_run,
        }
    }

    fn read_json(&self, relative_path: &str) -> Value {
        let raw = self.read_text(relative_path);
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
            "mutagen-harness-{name}-{}-{nanos}-{attempt}",
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
