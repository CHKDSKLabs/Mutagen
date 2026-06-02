use mutagen_core::notifications::{NotificationKind, StopCondition};
use mutagen_core::structural::{
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
### L1-orders-001 — 2026-04-23
Completed the aggregate and tests.
",
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
### L1-orders-001 — 2026-04-23
Completed the aggregate and tests.
",
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
fn structural_check_fails_when_state_update_block_is_missing_slice_marker() {
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
completed without naming the slice
",
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
            && finding.detail.contains("must contain a slice marker")
    }));
}

// A bold `**State Update**` marker still gets the slice rejected — but since
// L1-Harness-005 it's rejected by the *parser*, not the required-section gate.
// Two independent mechanisms touch this section now: `required_section_present`
// accepts the bold line as a heading equivalent (so no "missing required
// heading" finding), while the State Update block parser still hunts for a real
// `#`-prefixed heading + slice marker and bails when it only finds bold. The old
// version of this test asserted the required_section finding that no longer
// fires. Keep the rejection guarantee; assert it against the right check.
#[test]
fn structural_check_rejects_bold_state_update_marker() {
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

**State Update**
- L1-orders-001 completed
",
    );

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Fail);

    // The bold marker satisfies the required-section gate now — prove that gate
    // stays quiet so we don't silently regress L1-Harness-005.
    assert!(
        !report.findings.iter().any(|finding| {
            finding.check == "required_section" && finding.detail.contains("State Update")
        }),
        "bold `**State Update**` should satisfy required_section since L1-Harness-005"
    );

    // ...and the State Update parser is the one that still draws blood.
    assert!(report.findings.iter().any(|finding| {
        finding.check == "state_block"
            && finding.severity == StructuralSeverity::Fail
            && finding.detail.contains("missing a `State Update` section")
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
### L1-orders-001 — 2026-04-23
Completed the aggregate and tests.
",
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

#[test]
fn structural_check_routes_to_persona_drift_on_blank_short_output() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_text(
        ".mutagen/state/author-output/L1-orders-001.md",
        "Acknowledged — that background grep was already superseded by the direct Grep call. No further action; the slice is halted at intake.",
    );

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Fail);
    assert_eq!(report.stop_condition, Some(StopCondition::PersonaDrift));
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].check, "persona_drift");
    assert!(report.findings[0].detail.contains("zero required sections"));
    assert_eq!(report.notifications.len(), 1);
    assert_eq!(
        report.notifications[0].event,
        NotificationKind::PersonaDrift
    );
}

#[test]
fn structural_check_does_not_flag_persona_drift_when_one_section_is_present() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_text(
        ".mutagen/state/author-output/L1-orders-001.md",
        "## 🛠️ Execution:\nshort but at least the contract is acknowledged.",
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
fn structural_check_accepts_joined_citation_pair_for_bare_id() {
    // 2026-05-10 traces_to_drift postmortem: Chaplin wrote `DSD-621/622` and
    // the cited ID DSD-622 was missed by a plain substring scan. Here the
    // fixture cites bare `ADR-0001`; the author writes only the joined form
    // `ADR-0000/0001`. With the loosened citation_present helper, this should
    // pass — no traces_to_drift finding for ADR-0001.
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
- ADR-0000/0001
- [DSD-001]

## State Update
### L1-orders-001 — 2026-04-23
Completed the aggregate and tests.
",
    );

    let report = structural_check(workspace.structural_options("L1-orders-001"));

    assert_eq!(report.verdict, StructuralVerdict::Pass);
    assert!(
        !report.findings.iter().any(|f| f.check == "traces_to_drift"),
        "joined-pair citation form should not trigger traces_to_drift; got: {:?}",
        report.findings
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
            "mutagen-harness-{name}-structural-{}-{nanos}-{attempt}",
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
