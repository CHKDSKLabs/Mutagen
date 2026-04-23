use mutagen_harness::notifications::{NotificationKind, StopCondition};
use mutagen_harness::structural::{
    StructuralCheckOptions, StructuralSeverity, StructuralVerdict, structural_check,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn structural_check_passes_on_well_formed_author_output() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_text(
        ".mutagen/state/author-output/L1-orders-001.md",
        "# Intake Report

## 🛠️ Execution:
Built the aggregate and tests.

## Code Artifacts
- src/orders/aggregate.rs
- tests/orders/aggregate_tests.rs

## ISC Upholding Map
- [ISC-001]

## Verification Artifacts
- cargo test
- [FR-001]
- ADR-0001
- [DSD-001]

## State Update
- L1-orders-001 completed
",
    );
    workspace.write_text(
        "project_state.md",
        "# Project State\n\n- L1-orders-001 completed and recorded.\n",
    );

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Pass);
    assert!(report.findings.is_empty());
    assert_eq!(report.loc, serde_json::json!({}));
    assert_eq!(report.stop_condition, None);
    assert!(report.notifications.is_empty());
}

#[test]
fn structural_check_fails_when_required_section_is_missing() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_text(
        ".mutagen/state/author-output/L1-orders-001.md",
        "# Intake Report

## 🛠️ Execution:
Built the aggregate and tests.

## Code Artifacts
- src/orders/aggregate.rs

## ISC Upholding Map
- [ISC-001]

## Verification Artifacts
- cargo test
- [FR-001]
- ADR-0001
- [DSD-001]
",
    );
    workspace.write_text(
        "project_state.md",
        "# Project State\n\n- L1-orders-001 completed and recorded.\n",
    );

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Fail);
    assert_eq!(
        report.stop_condition,
        Some(StopCondition::StructuralFailure)
    );
    assert_eq!(report.notifications.len(), 1);
    assert_eq!(
        report.notifications[0].event,
        NotificationKind::StructuralFail
    );
    assert!(report.findings.iter().any(|finding| {
        finding.check == "required_section"
            && finding.severity == StructuralSeverity::Fail
            && finding.detail.contains("State Update")
    }));
}

#[test]
fn structural_check_fails_when_cited_ids_are_missing_from_author_output() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_text(
        ".mutagen/state/author-output/L1-orders-001.md",
        "# Intake Report

## 🛠️ Execution:
Built the aggregate and tests.

## Code Artifacts
- src/orders/aggregate.rs

## ISC Upholding Map
- no isc ids here

## Verification Artifacts
- cargo test

## State Update
- L1-orders-001 completed
",
    );
    workspace.write_text(
        "project_state.md",
        "# Project State\n\n- L1-orders-001 completed and recorded.\n",
    );

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Fail);
    assert_eq!(
        report.stop_condition,
        Some(StopCondition::StructuralFailure)
    );
    assert_eq!(
        report.notifications[0].event,
        NotificationKind::StructuralFail
    );
    assert!(report.findings.iter().any(|finding| {
        finding.check == "traces_to_drift"
            && finding.severity == StructuralSeverity::Fail
            && finding.detail.contains("[FR-001]")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check == "traces_to_drift"
            && finding.severity == StructuralSeverity::Fail
            && finding.detail.contains("ADR-0001")
    }));
}

#[test]
fn structural_check_fails_when_state_update_is_missing_from_context_file() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_text(
        ".mutagen/state/author-output/L1-orders-001.md",
        "# Intake Report

## 🛠️ Execution:
Built the aggregate and tests.

## Code Artifacts
- src/orders/aggregate.rs

## ISC Upholding Map
- [ISC-001]

## Verification Artifacts
- cargo test
- [FR-001]
- ADR-0001
- [DSD-001]

## State Update
- L1-orders-001 completed
",
    );
    workspace.write_text(
        "project_state.md",
        "# Project State\n\n- no slice marker here.\n",
    );

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Fail);
    assert_eq!(
        report.stop_condition,
        Some(StopCondition::StructuralFailure)
    );
    assert_eq!(
        report.notifications[0].event,
        NotificationKind::StructuralFail
    );
    assert!(report.findings.iter().any(|finding| {
        finding.check == "state_block"
            && finding.severity == StructuralSeverity::Fail
            && finding
                .detail
                .contains("State Update block likely not appended")
    }));
}

#[test]
fn structural_check_fails_when_loc_exceeds_hard_gate() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_text(
        ".mutagen/state/author-output/L1-orders-001.md",
        "# Intake Report

## 🛠️ Execution:
Built the aggregate and tests.

## Code Artifacts
- src/orders/aggregate.rs

## ISC Upholding Map
- [ISC-001]

## Verification Artifacts
- cargo test
- [FR-001]
- ADR-0001
- [DSD-001]

## State Update
- L1-orders-001 completed
",
    );
    workspace.write_text(
        "project_state.md",
        "# Project State\n\n- L1-orders-001 completed and recorded.\n",
    );
    workspace.write_text(
        "slice_loc_stub.sh",
        "#!/usr/bin/env bash\nprintf '{\"slice\":\"%s\",\"over_target_pct\":121,\"target\":300}\\n' \"$1\"\n",
    );

    let mut options = workspace.structural_options("L1-orders-001");
    options.loc_script_path = PathBuf::from("slice_loc_stub.sh");

    let report = structural_check(options);

    assert_eq!(report.verdict, StructuralVerdict::Fail);
    assert_eq!(report.loc["over_target_pct"], 121);
    assert_eq!(
        report.stop_condition,
        Some(StopCondition::StructuralFailure)
    );
    assert_eq!(
        report.notifications[0].event,
        NotificationKind::StructuralFail
    );
    assert!(report.findings.iter().any(|finding| {
        finding.check == "loc_overrun"
            && finding.severity == StructuralSeverity::Fail
            && finding.detail.contains("exceeds 120% hard gate")
    }));
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

    env::temp_dir().join(format!(
        "mutagen-harness-{name}-structural-{}-{nanos}",
        std::process::id()
    ))
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
