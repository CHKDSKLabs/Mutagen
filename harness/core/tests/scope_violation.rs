use mutagen_core::adapter::HostKind;
use mutagen_core::notifications::{NotificationKind, StopCondition};
use mutagen_core::queue::SliceStatus;
use mutagen_core::runtime::{PrepareNextOptions, prepare_next};
use mutagen_core::scope_violation::{ScopeViolationOptions, scope_violation};
use mutagen_core::state::Stage;
use mutagen_core::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

#[test]
fn scope_violation_escalates_current_slice_and_enriches_report() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.prepare_claimed_slice();
    workspace.write_json(
        ".mutagen/state/scope-violation.json",
        &json!({
            "ts": "2026-04-23T15:00:00Z",
            "decision": "deny",
            "class": "out_of_scope",
            "tool_name": "Write",
            "path": "src/shared/rogue.rs",
            "reason": "write outside active slice scope blocked",
            "message": "blocked"
        }),
    );

    let result = scope_violation(workspace.scope_violation_options())
        .expect("scope-violation should succeed");

    assert_eq!(result.stop_condition, StopCondition::ScopeViolation);
    assert!(result.queue_updated);
    assert_eq!(result.slice_id.as_deref(), Some("L1-orders-001"));
    assert_eq!(result.status, Some(SliceStatus::Escalated));
    assert!(result.escalation_reason.contains("src/shared/rogue.rs"));
    assert_eq!(result.notifications.len(), 1);
    assert_eq!(
        result.notifications[0].event,
        NotificationKind::ScopeViolation
    );
    assert_eq!(result.violation.stage.as_deref(), Some("author"));
    assert_eq!(result.violation.active_agent.as_deref(), Some("Bebop"));
    assert_eq!(
        result.violation.title.as_deref(),
        Some("Create order aggregate")
    );

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "escalated");
    assert_eq!(
        queue["slices"][0]["escalation_reason"],
        result.escalation_reason
    );

    let violation = workspace.read_json(".mutagen/state/scope-violation.json");
    assert_eq!(violation["slice_id"], "L1-orders-001");
    assert_eq!(violation["stage"], "author");
    assert_eq!(violation["active_agent"], "Bebop");
}

#[test]
fn scope_violation_returns_note_when_queue_cannot_be_updated() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_json(
        ".mutagen/state/scope-violation.json",
        &json!({
            "ts": "2026-04-23T15:05:00Z",
            "decision": "deny",
            "class": "global",
            "tool_name": "Edit",
            "path": "templates/ADR-template.md",
            "reason": "design bundle blocked",
            "message": "blocked",
            "slice_id": "L9-missing-999",
            "stage": "author",
            "active_agent": "Bebop"
        }),
    );

    let mut options = workspace.scope_violation_options();
    options.queue_path = workspace.root.join("missing-queue.json");

    let result = scope_violation(options).expect("scope-violation should still succeed");

    assert_eq!(result.stop_condition, StopCondition::ScopeViolation);
    assert!(!result.queue_updated);
    assert!(
        result
            .queue_update_note
            .as_deref()
            .unwrap_or_default()
            .contains("queue file not found at"),
        "unexpected queue update note: {:?}",
        result.queue_update_note
    );
    assert_eq!(result.slice_id.as_deref(), Some("L9-missing-999"));
    assert_eq!(
        result.notifications[0].event,
        NotificationKind::ScopeViolation
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

        let workspace = Self { root: destination };
        support::write_queue_validation(&workspace.root);
        workspace
    }

    fn prepare_claimed_slice(&self) {
        prepare_next(PrepareNextOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            queue_validation_path: support::queue_validation_path(&self.root),
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

    fn scope_violation_options(&self) -> ScopeViolationOptions {
        ScopeViolationOptions {
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            active_state_path: self.root.join(".mutagen/state/active-slice.json"),
            violation_path: self.root.join(".mutagen/state/scope-violation.json"),
        }
    }

    fn write_json(&self, relative_path: &str, value: &Value) {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture write parent should exist");
        }

        let body = serde_json::to_string_pretty(value).expect("fixture JSON should serialize");
        fs::write(path, format!("{body}\n")).expect("fixture JSON should write");
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
            "mutagen-harness-{name}-scope-violation-{}-{nanos}-{attempt}",
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
