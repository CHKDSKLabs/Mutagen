use mutagen_harness::adapter::{HostKind, ParallelDispatchMode, ScopeEnforcementMode};
use mutagen_harness::notifications::StopCondition;
use mutagen_harness::queue::SliceQueue;
use mutagen_harness::runtime::{PrepareNextOptions, PrepareNextResult, prepare_next};
use mutagen_harness::validation::validate_queue;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn prepare_next_claims_ready_slice_and_writes_state_and_evidence() {
    let workspace = FixtureWorkspace::copy("basic_ready");

    let result =
        prepare_next(workspace.prepare_next_options(false)).expect("prepare-next should succeed");

    let evidence_bundle_path = match result {
        PrepareNextResult::Ready { prepared } => {
            assert_eq!(prepared.slice_id, "L1-orders-001");
            assert_eq!(prepared.author_agent, "Bebop");
            assert_eq!(prepared.layer, 1);
            assert_eq!(prepared.bounded_context, "orders");
            assert_eq!(
                prepared.objective,
                "Create the initial order aggregate and its tests."
            );
            assert!(prepared.review_required);
            assert_eq!(prepared.attempts, 0);
            assert_eq!(prepared.context_to_update, "project_state.md");
            assert_eq!(
                prepared.write_set,
                vec!["src/orders/**".to_string(), "tests/orders/**".to_string()]
            );
            assert!(prepared.adjacent_scope_allowed.is_empty());
            assert!(prepared.depends_on.is_empty());
            assert!(prepared.claimed);
            assert!(
                prepared
                    .degraded_capabilities
                    .contains(&"pre_write_scope_enforcement".to_string())
            );
            assert_eq!(
                prepared.host_profile.scope_enforcement,
                ScopeEnforcementMode::Advisory
            );
            assert_eq!(
                prepared.host_profile.parallel_dispatch,
                ParallelDispatchMode::SerialOnly
            );
            assert_eq!(prepared.host_profile.requested_max_parallel_slices, 3);
            assert_eq!(prepared.host_profile.effective_max_parallel_slices, 1);
            assert!(
                prepared
                    .host_profile
                    .degraded_features
                    .contains(&"parallel_dispatch".to_string())
            );
            prepared.evidence_bundle_path
        }
        other => panic!("expected ready result, got {other:?}"),
    };

    let active_state = workspace.read_json(".mutagen/state/active-slice.json");
    assert_eq!(active_state["slice_id"], "L1-orders-001");
    assert_eq!(active_state["author_agent"], "Bebop");
    assert_eq!(active_state["evidence_bundle_path"], evidence_bundle_path);

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "in_progress");
    assert_eq!(queue["slices"][1]["status"], "pending");

    let evidence = workspace.read_text(".mutagen/state/evidence/L1-orders-001.md");
    assert!(evidence.contains("## Evidence Bundle for L1-orders-001"));
    assert!(evidence.contains("#### [FR-001]"));
    assert!(evidence.contains("#### ADR-0001"));
    assert!(evidence.contains("#### OrderAggregate"));
    assert!(evidence.contains("#### [ISC-001]"));
    assert!(evidence.contains("#### [DSD-001]"));
}

#[test]
fn prepare_next_returns_stalled_when_dependencies_are_unmet() {
    let workspace = FixtureWorkspace::copy("stalled");

    let result = prepare_next(workspace.prepare_next_options(true))
        .expect("prepare-next should report stalled state");

    match result {
        PrepareNextResult::Stalled {
            blocked,
            stop_condition,
        } => {
            assert_eq!(blocked.len(), 1);
            assert_eq!(blocked[0].id, "L2-payments-001");
            assert_eq!(
                blocked[0].unmet_dependencies,
                vec!["L1-core-001".to_string()]
            );
            assert_eq!(stop_condition, StopCondition::QueueStalled);
        }
        other => panic!("expected stalled result, got {other:?}"),
    }
}

#[test]
fn prepare_next_returns_queue_clear_when_no_ready_candidates_remain() {
    let workspace = FixtureWorkspace::copy("queue_clear");

    let result = prepare_next(workspace.prepare_next_options(true))
        .expect("prepare-next should report queue clear");

    match result {
        PrepareNextResult::QueueClear {
            completed_count,
            stop_condition,
            notifications,
        } => {
            assert_eq!(completed_count, 1);
            assert_eq!(stop_condition, StopCondition::QueueClear);
            assert_eq!(notifications.len(), 1);
            assert_eq!(
                notifications[0].event,
                mutagen_harness::notifications::NotificationKind::QueueClear
            );
        }
        other => panic!("expected queue clear result, got {other:?}"),
    }
}

#[test]
fn prepare_next_fails_when_evidence_citation_cannot_be_resolved() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][0]["traces_to"]["prd"] = json!(["[FR-999]"]);
    workspace.write_json("slices/queue.json", &queue);

    let error = prepare_next(workspace.prepare_next_options(true))
        .expect_err("prepare-next should fail on unresolved evidence");

    assert!(error.to_string().contains("[FR-999]"));
}

#[test]
fn validate_queue_warns_when_target_loc_exceeds_default_budget() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][0]["target_loc"] = json!(450);
    workspace.write_json("slices/queue.json", &queue);

    let parsed_queue: SliceQueue =
        serde_json::from_value(queue).expect("fixture queue should deserialize");
    let report = validate_queue(&parsed_queue);

    assert!(report.ok);
    assert_eq!(report.error_count, 0);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "target_loc_above_default")
    );
}

#[test]
fn validate_queue_fails_on_unknown_dependencies() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][0]["depends_on"] = json!(["L9-missing-999"]);
    workspace.write_json("slices/queue.json", &queue);

    let parsed_queue: SliceQueue =
        serde_json::from_value(queue).expect("fixture queue should deserialize");
    let report = validate_queue(&parsed_queue);

    assert!(!report.ok);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.code == "unknown_dependency")
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
