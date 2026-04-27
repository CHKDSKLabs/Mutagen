use mutagen_harness::adapter::HostKind;
use mutagen_harness::queue_update::{UpdateSliceOptions, update_slice};
use mutagen_harness::runtime::{PrepareNextOptions, prepare_next};
use mutagen_harness::state::Stage;
use mutagen_harness::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn transition_author_dispatch_bumps_attempts_and_preserves_first_pass_scope() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    prepare_next(workspace.prepare_next_options(false)).expect("prepare-next should succeed");

    let result = transition_active_slice(workspace.transition_options(
        "L1-orders-001",
        Stage::Author,
        None,
        true,
        false,
    ))
    .expect("author transition should succeed");

    assert_eq!(result.stage, Stage::Author);
    assert_eq!(result.active_agent, "Bebop");
    assert_eq!(result.attempts, 1);
    assert_eq!(
        result.status,
        mutagen_harness::queue::SliceStatus::InProgress
    );
    assert!(
        result
            .allowed_write_globs
            .contains(&"src/orders/**".to_string())
    );
    assert!(
        result
            .allowed_write_globs
            .contains(&"tests/orders/**".to_string())
    );
    assert!(
        !result
            .allowed_write_globs
            .contains(&"project_state.md".to_string())
    );
    assert!(
        !result
            .allowed_write_globs
            .contains(&"infrastructure_state.md".to_string())
    );

    let active_state = workspace.read_json(".mutagen/state/active-slice.json");
    assert_eq!(active_state["stage"], "author");
    assert_eq!(active_state["active_agent"], "Bebop");
    assert_eq!(active_state["attempts"], 1);
    assert!(
        active_state["started_at_unix_ms"].as_u64().is_some(),
        "first author dispatch should stamp slice start time"
    );

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "in_progress");
    assert_eq!(queue["slices"][0]["attempts"], 1);
}

#[test]
fn transition_structural_stage_sets_karai_scope() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    prepare_next(workspace.prepare_next_options(false)).expect("prepare-next should succeed");

    let result = transition_active_slice(workspace.transition_options(
        "L1-orders-001",
        Stage::StructuralCheck,
        None,
        false,
        false,
    ))
    .expect("structural transition should succeed");

    assert_eq!(result.stage, Stage::StructuralCheck);
    assert_eq!(result.active_agent, "Karai");
    assert_eq!(result.allowed_write_globs, vec![".mutagen/state/**"]);

    let active_state = workspace.read_json(".mutagen/state/active-slice.json");
    assert_eq!(active_state["stage"], "structural_check");
    assert_eq!(active_state["active_agent"], "Karai");
    assert_eq!(
        active_state["allowed_write_globs"],
        json!([".mutagen/state/**"])
    );
}

#[test]
fn transition_review_stage_adds_security_scope_for_tatsu() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][0]["author_agent"] = json!("Tatsu");
    workspace.write_json("slices/queue.json", &queue);

    prepare_next(workspace.prepare_next_options(false)).expect("prepare-next should succeed");

    let result = transition_active_slice(workspace.transition_options(
        "L1-orders-001",
        Stage::Review,
        None,
        false,
        false,
    ))
    .expect("review transition should succeed");

    assert_eq!(result.stage, Stage::Review);
    assert_eq!(result.active_agent, "TigerClaw");
    assert!(
        result
            .allowed_write_globs
            .contains(&"tests/qa/security/**".to_string())
    );

    let active_state = workspace.read_json(".mutagen/state/active-slice.json");
    assert_eq!(active_state["stage"], "review");
    assert_eq!(active_state["active_agent"], "TigerClaw");
    assert!(
        active_state["allowed_write_globs"]
            .as_array()
            .expect("allowed_write_globs should be an array")
            .iter()
            .any(|value| value == "tests/qa/security/**")
    );
}

#[test]
fn transition_author_micro_correction_syncs_retry_scope_and_counter() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    prepare_next(workspace.prepare_next_options(false)).expect("prepare-next should succeed");

    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][0]["adjacent_scope_allowed"] = json!(["src/shared/**"]);
    workspace.write_json("slices/queue.json", &queue);

    update_slice(UpdateSliceOptions {
        queue_path: workspace.root.join("slices/queue.json"),
        slice_id: "L1-orders-001".to_string(),
        status: Some(mutagen_harness::queue::SliceStatus::BlockedRetry),
        attempts: Some(1),
        micro_corrections_used: None,
        karai_structural: None,
        bishop: None,
        tiger_claw: None,
        micro_correction: None,
        completed_at: None,
        clear_completed_at: false,
        escalation_reason: None,
        clear_escalation_reason: false,
        human_check_resolved_at: None,
        clear_human_check_resolved_at: false,
    })
    .expect("queue update should succeed");

    let result = transition_active_slice(workspace.transition_options(
        "L1-orders-001",
        Stage::Author,
        Some("Bebop".to_string()),
        false,
        true,
    ))
    .expect("micro-correction transition should succeed");

    assert_eq!(result.stage, Stage::Author);
    assert_eq!(result.active_agent, "Bebop");
    assert_eq!(result.attempts, 1);
    assert_eq!(result.micro_corrections_used, 1);
    assert!(
        result
            .allowed_write_globs
            .contains(&"src/shared/**".to_string())
    );

    let active_state = workspace.read_json(".mutagen/state/active-slice.json");
    assert_eq!(active_state["micro_corrections_used"], 1);
    assert!(
        active_state["allowed_write_globs"]
            .as_array()
            .expect("allowed_write_globs should be an array")
            .iter()
            .any(|value| value == "src/shared/**")
    );

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "in_progress");
    assert_eq!(queue["slices"][0]["attempts"], 1);
    assert_eq!(queue["slices"][0]["micro_corrections_used"], 1);
    assert_eq!(queue["slices"][0]["verdicts"]["micro_corrections_used"], 1);
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

    fn prepare_next_options(&self, dry_run: bool) -> PrepareNextOptions {
        PrepareNextOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            workflow_config_path: self.root.join(".claude/workflow.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            host: HostKind::Codex,
            dry_run,
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
            "mutagen-harness-{name}-transition-active-{}-{nanos}-{attempt}",
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
