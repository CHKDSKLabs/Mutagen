use mutagen_core::structural::{StructuralCheckOptions, structural_check};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn bold_marker_satisfies_required_section() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_author_output(&author_output_with(|marker| format!("**{marker}**")));

    let report = structural_check(workspace.structural_options());

    let missing: Vec<&str> = report
        .findings
        .iter()
        .filter(|finding| finding.check == "required_section")
        .map(|finding| finding.detail.as_str())
        .collect();
    assert!(
        missing.is_empty(),
        "bold-line markers should satisfy required_section_present; got: {missing:?}"
    );
}

#[test]
fn h3_marker_satisfies_required_section() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_author_output(&author_output_with(|marker| format!("### {marker}")));

    let report = structural_check(workspace.structural_options());

    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.check == "required_section"),
        "h3 headings preserve the prior depth-tolerant behavior"
    );
}

#[test]
fn canonical_h2_marker_still_works() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_author_output(&author_output_with(|marker| format!("## {marker}")));

    let report = structural_check(workspace.structural_options());

    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.check == "required_section"),
    );
}

#[test]
fn whitespace_padded_bold_marker_does_not_satisfy() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    // Use canonical h2 for everything except Intake Report, which gets the
    // whitespace-padded form we want to keep on the wrong side of the gate.
    let body = author_output_with(|marker| {
        if marker == "Intake Report" {
            format!("** {marker} **")
        } else {
            format!("## {marker}")
        }
    });
    workspace.write_author_output(&body);

    let report = structural_check(workspace.structural_options());

    let intake_finding = report.findings.iter().find(|finding| {
        finding.check == "required_section" && finding.detail.contains("Intake Report")
    });
    assert!(
        intake_finding.is_some(),
        "`** Intake Report **` must be rejected — bold matching is exact-on-the-marker"
    );
}

#[test]
fn embedded_bold_inside_prose_does_not_satisfy() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let body = author_output_with(|marker| {
        if marker == "Intake Report" {
            format!("**{marker}** is great")
        } else {
            format!("## {marker}")
        }
    });
    workspace.write_author_output(&body);

    let report = structural_check(workspace.structural_options());

    let intake_finding = report.findings.iter().find(|finding| {
        finding.check == "required_section" && finding.detail.contains("Intake Report")
    });
    assert!(
        intake_finding.is_some(),
        "`**Intake Report** is great` must be rejected — bold inside prose is not a heading"
    );
}

#[test]
fn mixed_h2_h3_and_bold_markers_all_satisfy() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    let body = author_output_with(|marker| match marker {
        "Intake Report" => format!("## {marker}"),
        "Code Artifacts" => format!("### {marker}"),
        "ISC Upholding Map" => format!("**{marker}**"),
        "Verification Artifacts" => format!("### {marker}"),
        // Keep State Update as canonical h2 so the State Update parser
        // (which still requires `##`) doesn't emit a state_block finding
        // unrelated to the heading-recognition concern under test.
        _ => format!("## {marker}"),
    });
    workspace.write_author_output(&body);

    let report = structural_check(workspace.structural_options());

    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.check == "required_section"),
        "any mix of h2 / h3 / bold-line markers should satisfy required_section_present; got {:?}",
        report.findings
    );
}

fn author_output_with<F: Fn(&str) -> String>(format_marker: F) -> String {
    let exec = format_marker("🛠️ Execution:");
    let intake = format_marker("Intake Report");
    let code = format_marker("Code Artifacts");
    let isc = format_marker("ISC Upholding Map");
    let verify = format_marker("Verification Artifacts");
    let state = format_marker("State Update");
    format!(
        "{exec}\nBuilt the aggregate and tests.\n\n\
         {intake}\n- domain fit: OK\n\n\
         {code}\n- src/orders/aggregate.rs\n\n\
         {isc}\n- [ISC-001]\n\n\
         {verify}\n- cargo test\n- [FR-001]\n- ADR-0001\n- [DSD-001]\n\n\
         {state}\n### L1-orders-001 — 2026-05-14\nCompleted the aggregate.\n",
    )
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

    fn structural_options(&self) -> StructuralCheckOptions {
        StructuralCheckOptions {
            slice_id: "L1-orders-001".to_string(),
            workspace_root: self.root.clone(),
            queue_path: self.root.join("slices/queue.json"),
            author_output_dir: self.root.join(".mutagen/state/author-output"),
            loc_script_path: self.root.join("missing_slice_loc.sh"),
        }
    }

    fn write_author_output(&self, body: &str) {
        let path = self
            .root
            .join(".mutagen/state/author-output/L1-orders-001.md");
        fs::create_dir_all(path.parent().unwrap()).expect("author output parent should exist");
        fs::write(path, body).expect("author output should write");
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
            "mutagen-harness-{name}-bold-headers-{}-{nanos}-{attempt}",
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
