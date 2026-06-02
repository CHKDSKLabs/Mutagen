// Contract spec for the persona-drift gate. Each test pins a row in the
// (sections_present, trimmed_len) matrix that the structural check walks
// after the 2026-05-12 L4-Workflow-001 incident. The gate now classifies on
// canonical-heading presence alone; the size hint below is forensic, not
// load-bearing — cross-ref `PERSONA_DRIFT_CHAR_THRESHOLD` in
// `harness/core/src/structural.rs`.
const SIZE_HINT: usize = 600;

use mutagen_core::notifications::{NotificationKind, StopCondition};
use mutagen_core::structural::{StructuralCheckOptions, StructuralVerdict, structural_check};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn empty_author_classifies_as_persona_drift() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_text(".mutagen/state/author-output/L1-orders-001.md", "");

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Fail);
    assert_eq!(report.stop_condition, Some(StopCondition::PersonaDrift));
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].check, "persona_drift");
    assert_eq!(report.notifications.len(), 1);
    assert_eq!(
        report.notifications[0].event,
        NotificationKind::PersonaDrift
    );
}

#[test]
fn sub_threshold_unbordered_output_classifies_as_persona_drift() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let body = "Acknowledged — halting at intake; nothing else to report.";
    assert!(body.len() < SIZE_HINT, "fixture must stay sub-threshold");
    workspace.write_text(".mutagen/state/author-output/L1-orders-001.md", body);

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Fail);
    assert_eq!(report.stop_condition, Some(StopCondition::PersonaDrift));
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].check, "persona_drift");
}

#[test]
fn sub_threshold_with_one_canonical_heading_is_contract_violation_not_drift() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_text(
        ".mutagen/state/author-output/L1-orders-001.md",
        "## 🛠️ Execution:\nshort but at least one heading is here.",
    );

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Fail);
    assert_eq!(
        report.stop_condition,
        Some(StopCondition::StructuralFailure)
    );
    assert!(!report.findings.iter().any(|f| f.check == "persona_drift"));
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.check == "required_section")
    );
}

#[test]
fn supra_threshold_without_any_canonical_heading_classifies_as_persona_drift() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let mut body = String::with_capacity(SIZE_HINT * 2);
    while body.len() < SIZE_HINT + 200 {
        body.push_str(
            "This author rambled on about the slice in prose with no canonical headings at all. ",
        );
    }
    assert!(body.len() > SIZE_HINT, "fixture must clear the size hint");
    workspace.write_text(".mutagen/state/author-output/L1-orders-001.md", &body);

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Fail);
    assert_eq!(report.stop_condition, Some(StopCondition::PersonaDrift));
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].check, "persona_drift");
    assert_eq!(report.notifications.len(), 1);
    assert_eq!(
        report.notifications[0].event,
        NotificationKind::PersonaDrift
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
            "mutagen-harness-{name}-persona-drift-{}-{nanos}-{attempt}",
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
