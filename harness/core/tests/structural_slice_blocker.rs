// Contract spec for the Slice Blocker gate. Tracking the 2026-05-06
// L1-Infra-005 postmortem: an author whose declared write_set is
// insufficient to satisfy its own verification commands should be able to
// surface a typed refusal instead of getting buried in "missing required
// heading" findings. Three reason tokens are recognized; anything else is
// still escalated, but as `unknown_reason`.

use mutagen_core::notifications::{NotificationKind, StopCondition};
use mutagen_core::structural::{StructuralCheckOptions, StructuralVerdict, structural_check};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn slice_blocker_with_recognized_reason_routes_to_blocker_escalation() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let body = "## Slice Blocker\n\
                Cannot proceed: declared write_set excludes harness/service/src/lib.rs \
                but the slice's verification commands require touching it. \
                reason: predecessor_repair_required\n";
    workspace.write_text(".mutagen/state/author-output/L1-orders-001.md", body);

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Fail);
    assert_eq!(report.stop_condition, Some(StopCondition::SliceBlocker));
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].check, "slice_blocker");
    assert!(
        report.findings[0]
            .detail
            .contains("predecessor_repair_required"),
        "finding should name the reason token"
    );
    assert_eq!(report.notifications.len(), 1);
    assert_eq!(
        report.notifications[0].event,
        NotificationKind::SliceBlocker
    );
}

#[test]
fn slice_blocker_supports_all_three_canonical_reasons() {
    for reason in [
        "scope_amendment_request",
        "predecessor_repair_required",
        "contradictory_traces",
    ] {
        let workspace = FixtureWorkspace::copy("basic_ready");
        let body = format!("## Slice Blocker\nreason: {reason}\nBody narrative below.\n");
        workspace.write_text(".mutagen/state/author-output/L1-orders-001.md", &body);

        let report = structural_check(workspace.structural_options("L1-orders-001"));

        assert_eq!(report.stop_condition, Some(StopCondition::SliceBlocker));
        assert!(
            report.findings[0].detail.contains(reason),
            "reason `{reason}` should surface in the finding detail"
        );
    }
}

#[test]
fn slice_blocker_with_unrecognized_reason_still_escalates_but_marked_unknown() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let body = "## Slice Blocker\nreason: cargo_lock_drift\nNarrative.\n";
    workspace.write_text(".mutagen/state/author-output/L1-orders-001.md", body);

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.stop_condition, Some(StopCondition::SliceBlocker));
    assert!(report.findings[0].detail.contains("unknown_reason"));
}

#[test]
fn slice_blocker_at_any_heading_depth_is_recognized() {
    for prefix in ["##", "###", "####"] {
        let workspace = FixtureWorkspace::copy("basic_ready");
        let body = format!(
            "Some prose first.\n\n{prefix} Slice Blocker\nreason: contradictory_traces\nBody."
        );
        workspace.write_text(".mutagen/state/author-output/L1-orders-001.md", &body);

        let report = structural_check(workspace.structural_options("L1-orders-001"));

        assert_eq!(
            report.stop_condition,
            Some(StopCondition::SliceBlocker),
            "depth `{prefix}` should still trip the blocker gate"
        );
    }
}

#[test]
fn slice_blocker_substring_in_prose_does_not_trip_the_gate() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    // No heading anchor — just the phrase mentioned in prose. We want the
    // persona-drift gate to claim this one, not the blocker gate.
    let body = "## 🛠️ Execution:\n\
                The author noted that a slice blocker would have been the cleaner \
                exit, but proceeded anyway. predecessor_repair_required.\n\
                ## Intake Report\nbody\n## Code Artifacts\nbody\n\
                ## ISC Upholding Map\nbody\n## Verification Artifacts\nbody\n\
                ## State Update\n```\n### L1-orders-001 — 2026-05-18\nbody\n```\n";
    workspace.write_text(".mutagen/state/author-output/L1-orders-001.md", body);

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_ne!(
        report.stop_condition,
        Some(StopCondition::SliceBlocker),
        "free-text mention of slice blocker must not promote to blocker escalation"
    );
}

#[test]
fn slice_blocker_takes_precedence_over_persona_drift_on_short_output() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    // Short body with zero canonical headings, but with a real Slice Blocker
    // heading. Pre-fix this would have been persona_drift; post-fix it's a
    // typed blocker.
    let body = "## Slice Blocker\nreason: scope_amendment_request\nshort.\n";
    workspace.write_text(".mutagen/state/author-output/L1-orders-001.md", body);

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.stop_condition, Some(StopCondition::SliceBlocker));
    assert!(!report.findings.iter().any(|f| f.check == "persona_drift"));
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

    fn structural_options(&self, slice_id: &str) -> StructuralCheckOptions {
        StructuralCheckOptions {
            slice_id: slice_id.to_string(),
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            author_output_dir: self.root.join(".mutagen/state/author-output"),
            loc_script_path: self.root.join("missing_slice_loc.sh"),
        }
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

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();

    for attempt in 0..1024 {
        let path = env::temp_dir().join(format!(
            "mutagen-harness-{name}-slice-blocker-{}-{nanos}-{attempt}",
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
