use mutagen_harness::adapter::HostKind;
use mutagen_harness::amend_scope::{
    AmendScopeOptions, AmendmentDecision, DenyClass, MutationKind, amend_scope,
};
use mutagen_harness::runtime::{PrepareNextOptions, prepare_next};
use mutagen_harness::state::Stage;
use mutagen_harness::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn amend_scope_allows_author_stage_widening_within_agent_domain() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_author_stage();

    let result = amend_scope(workspace.amend_scope_options(
        vec!["src/orders/support/**".to_string()],
        MutationKind::Modify,
        "Need a small helper beside the aggregate.".to_string(),
    ))
    .expect("amend-scope should succeed");

    assert_eq!(result.decision, AmendmentDecision::Allow);
    assert_eq!(
        result.added_globs,
        vec!["src/orders/support/**".to_string()]
    );
    assert!(
        result
            .allowed_write_globs
            .contains(&"src/orders/support/**".to_string())
    );
    assert!(!result.justification_gap);

    let active_state = workspace.read_json(".mutagen/state/active-slice.json");
    assert!(
        active_state["allowed_write_globs"]
            .as_array()
            .expect("allowed_write_globs should be an array")
            .iter()
            .any(|value| value == "src/orders/support/**")
    );
    assert_eq!(
        active_state["amendments"][0]["added"][0],
        "src/orders/support/**"
    );

    let log = workspace.read_text(".mutagen/state/amendments.jsonl");
    assert!(log.contains("\"decision\":\"allow\""));
    assert!(log.contains("src/orders/support/**"));
}

#[test]
fn amend_scope_denies_global_paths() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_author_stage();

    let result = amend_scope(workspace.amend_scope_options(
        vec!["templates/ADR-template.md".to_string()],
        MutationKind::Modify,
        "Wanted to patch the template mid-slice.".to_string(),
    ))
    .expect("amend-scope should succeed");

    assert_eq!(result.decision, AmendmentDecision::Deny);
    assert_eq!(result.class, Some(DenyClass::Global));
    assert_eq!(result.matched_rule.as_deref(), Some("templates/**"));

    let active_state = workspace.read_json(".mutagen/state/active-slice.json");
    assert!(
        active_state["allowed_write_globs"]
            .as_array()
            .expect("allowed_write_globs should be an array")
            .iter()
            .all(|value| value != "templates/ADR-template.md")
    );

    let log = workspace.read_text(".mutagen/state/amendments.jsonl");
    assert!(log.contains("\"decision\":\"deny\""));
    assert!(log.contains("\"class\":\"global\""));
}

#[test]
fn amend_scope_denies_stage_fidelity_mismatches() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_author_stage();

    let result = amend_scope(workspace.amend_scope_options(
        vec!["reviews/L1-orders-001/**".to_string()],
        MutationKind::Create,
        "Need to pre-create the review folder.".to_string(),
    ))
    .expect("amend-scope should succeed");

    assert_eq!(result.decision, AmendmentDecision::Deny);
    assert_eq!(result.class, Some(DenyClass::StageFidelity));
    assert!(result.suggested_next_step.contains("review"));
}

#[test]
fn stage_transition_clears_current_stage_amendments() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_author_stage();

    amend_scope(workspace.amend_scope_options(
        vec!["src/orders/support/**".to_string()],
        MutationKind::Modify,
        "Need a small helper beside the aggregate.".to_string(),
    ))
    .expect("amend-scope should succeed");

    transition_active_slice(TransitionActiveSliceOptions {
        queue_path: workspace.root.join("slices/queue.json"),
        active_state_path: workspace.root.join(".mutagen/state/active-slice.json"),
        slice_id: "L1-orders-001".to_string(),
        stage: Stage::StructuralCheck,
        active_agent: None,
        bump_attempts: false,
        bump_micro_corrections: false,
    })
    .expect("stage transition should succeed");

    let active_state = workspace.read_json(".mutagen/state/active-slice.json");
    assert_eq!(active_state["stage"], "structural_check");
    assert_eq!(active_state["amendments"], Value::Null);
    assert_eq!(
        active_state["allowed_write_globs"],
        json!([".mutagen/state/**"])
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

    fn prepare_author_stage(&self) {
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
            stage: Stage::Author,
            active_agent: None,
            bump_attempts: true,
            bump_micro_corrections: false,
        })
        .expect("author transition should succeed");
    }

    fn amend_scope_options(
        &self,
        requested_globs: Vec<String>,
        mutation_kind: MutationKind,
        reason: String,
    ) -> AmendScopeOptions {
        AmendScopeOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            amendments_log_path: self.root.join(".mutagen/state/amendments.jsonl"),
            requested_globs,
            mutation_kind,
            reason,
        }
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
            "mutagen-harness-{name}-amend-scope-{}-{nanos}-{attempt}",
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
