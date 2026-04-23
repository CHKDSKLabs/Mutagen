use mutagen_harness::adapter::{HostKind, ParallelDispatchMode};
use mutagen_harness::cohort::{
    DeferredReason, PrepareCohortOptions, PrepareCohortResult, prepare_cohort,
};
use mutagen_harness::notifications::StopCondition;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn prepare_cohort_selects_disjoint_ready_siblings_in_queue_order() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][1]["layer"] = json!(1);
    queue["slices"][1]["depends_on"] = json!([]);
    queue["slices"][1]["context_to_update"] = json!("infrastructure_state.md");
    workspace.write_json("slices/queue.json", &queue);

    let result = prepare_cohort(workspace.prepare_cohort_options(HostKind::Claude, false))
        .expect("prepare-cohort should succeed");

    match result {
        PrepareCohortResult::Ready {
            cohort_layer,
            effective_max_parallel_slices,
            host_profile,
            prepared,
            cohort,
            deferred,
            ..
        } => {
            assert_eq!(cohort_layer, 1);
            assert_eq!(effective_max_parallel_slices, 3);
            assert_eq!(
                host_profile.parallel_dispatch,
                ParallelDispatchMode::BoundedCohort
            );
            assert!(prepared);
            assert_eq!(cohort.len(), 2);
            assert_eq!(cohort[0].slice_id, "L1-orders-001");
            assert_eq!(cohort[1].slice_id, "L2-orders-002");
            assert!(deferred.is_empty());

            let evidence_one = workspace.read_text(".mutagen/state/evidence/L1-orders-001.md");
            let evidence_two = workspace.read_text(".mutagen/state/evidence/L2-orders-002.md");
            assert!(evidence_one.contains("## Evidence Bundle for L1-orders-001"));
            assert!(evidence_two.contains("## Evidence Bundle for L2-orders-002"));
        }
        other => panic!("expected ready result, got {other:?}"),
    }
}

#[test]
fn prepare_cohort_keeps_same_context_siblings_when_write_sets_are_disjoint() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][1]["layer"] = json!(1);
    queue["slices"][1]["depends_on"] = json!([]);
    workspace.write_json("slices/queue.json", &queue);

    let result = prepare_cohort(workspace.prepare_cohort_options(HostKind::Claude, true))
        .expect("prepare-cohort should succeed");

    match result {
        PrepareCohortResult::Ready {
            cohort, deferred, ..
        } => {
            assert_eq!(cohort.len(), 2);
            assert_eq!(cohort[0].slice_id, "L1-orders-001");
            assert_eq!(cohort[1].slice_id, "L2-orders-002");
            assert!(deferred.is_empty());
        }
        other => panic!("expected ready result, got {other:?}"),
    }
}

#[test]
fn prepare_cohort_defers_conflicting_write_sets_and_layer_mismatches() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][1]["layer"] = json!(1);
    queue["slices"][1]["depends_on"] = json!([]);
    queue["slices"][1]["write_set"] = json!(["src/orders/api/**", "tests/http/**"]);
    queue["slices"][1]["context_to_update"] = json!("infrastructure_state.md");
    let third_slice = json!({
      "id": "L2-billing-003",
      "title": "Billing UI",
      "phase": "phase_1",
      "status": "pending",
      "author_agent": "Bebop",
      "layer": 2,
      "bounded_context": "billing",
      "target_loc": 300,
      "objective": "Build a billing UI slice.",
      "context_to_update": "infrastructure_state.md",
      "implementation_details": [
        "Create billing UI components.",
        "Keep the change inside billing paths."
      ],
      "review_required": true,
      "attempts": 0,
      "micro_corrections_used": 0,
      "depends_on": [],
      "adjacent_scope_allowed": [],
      "write_set": ["src/billing/**", "tests/billing/**"],
      "traces_to": {
        "prd": ["[FR-001]"],
        "adr": ["ADR-0001"],
        "ddd": ["OrderAggregate"],
        "isc": ["[ISC-001]"],
        "dsd": ["[DSD-001]"]
      },
      "verification_steps": {
        "acceptance": "cargo test",
        "isc_detection": "Run billing contract tests",
        "dsd_conformance": "Check billing naming conventions"
      },
      "human_check_needed": {
        "required": false,
        "reason": "",
        "resolved_at": null
      }
    });
    queue["slices"]
        .as_array_mut()
        .expect("slices should be array")
        .push(third_slice);
    workspace.write_json("slices/queue.json", &queue);

    let result = prepare_cohort(workspace.prepare_cohort_options(HostKind::Claude, true))
        .expect("prepare-cohort should succeed");

    match result {
        PrepareCohortResult::Ready {
            cohort, deferred, ..
        } => {
            assert_eq!(cohort.len(), 1);
            assert_eq!(cohort[0].slice_id, "L1-orders-001");
            assert_eq!(deferred.len(), 2);

            let conflict = deferred
                .iter()
                .find(|entry| entry.slice_id == "L2-orders-002")
                .expect("conflicting slice should be deferred");
            assert_eq!(conflict.reason, DeferredReason::WriteSetConflict);
            assert_eq!(
                conflict.conflicting_slice_id.as_deref(),
                Some("L1-orders-001")
            );

            let layer_mismatch = deferred
                .iter()
                .find(|entry| entry.slice_id == "L2-billing-003")
                .expect("layer mismatch slice should be deferred");
            assert_eq!(layer_mismatch.reason, DeferredReason::LayerMismatch);
        }
        other => panic!("expected ready result, got {other:?}"),
    }
}

#[test]
fn prepare_cohort_returns_serial_only_when_host_cannot_parallelize() {
    let workspace = FixtureWorkspace::copy("basic_ready");

    let result = prepare_cohort(workspace.prepare_cohort_options(HostKind::Codex, true))
        .expect("prepare-cohort should succeed");

    match result {
        PrepareCohortResult::SerialOnly {
            host,
            host_profile,
            message,
        } => {
            assert_eq!(host, HostKind::Codex);
            assert_eq!(
                host_profile.parallel_dispatch,
                ParallelDispatchMode::SerialOnly
            );
            assert!(message.contains("serial_only"));
        }
        other => panic!("expected serial_only result, got {other:?}"),
    }
}

#[test]
fn prepare_cohort_returns_stalled_when_no_ready_siblings_exist() {
    let workspace = FixtureWorkspace::copy("stalled");
    workspace.write_json(
        ".claude/workflow.json",
        &json!({
            "pipeline_mode": "full",
            "max_parallel_slices": 3,
            "review": {
                "max_retries": 2,
                "max_micro_corrections": 1
            }
        }),
    );

    let result = prepare_cohort(workspace.prepare_cohort_options(HostKind::Claude, true))
        .expect("prepare-cohort should report stalled");

    match result {
        PrepareCohortResult::Stalled {
            blocked,
            stop_condition,
        } => {
            assert_eq!(blocked.len(), 1);
            assert_eq!(blocked[0].id, "L2-payments-001");
            assert_eq!(stop_condition, StopCondition::QueueStalled);
        }
        other => panic!("expected stalled result, got {other:?}"),
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

    fn prepare_cohort_options(&self, host: HostKind, dry_run: bool) -> PrepareCohortOptions {
        PrepareCohortOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            workflow_config_path: self.root.join(".claude/workflow.json"),
            host,
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
            "mutagen-harness-{name}-cohort-{}-{nanos}-{attempt}",
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
