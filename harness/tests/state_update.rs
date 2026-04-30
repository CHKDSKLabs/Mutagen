use mutagen_harness::state_update::{
    ApplyStateUpdateOptions, apply_state_update_for_slice, parse_state_update,
};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn parse_state_update_extracts_fenced_markdown_block() {
    let output = r#"### 🛠️ Execution: L1-orders-001
#### Intake Report
- Domain fit: standard execution
#### State Update
```markdown
### L1-orders-001 — 2026-04-23
**Artifacts:** src/orders/aggregate.rs
```
"#;

    let parsed = parse_state_update(output, "L1-orders-001")
        .expect("state update should parse from fenced block");

    assert_eq!(parsed.marker, "### L1-orders-001 — 2026-04-23");
    assert!(
        parsed
            .body
            .contains("**Artifacts:** src/orders/aggregate.rs")
    );
    assert!(!parsed.body.contains("```"));
}

#[test]
fn apply_state_update_for_slice_appends_once() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.write_text(
        ".mutagen/state/author-output/L1-orders-001.md",
        r#"### 🛠️ Execution: L1-orders-001
#### Intake Report
- Domain fit: standard execution
#### State Update
### L1-orders-001 — 2026-04-23
**Artifacts:** src/orders/aggregate.rs
"#,
    );
    workspace.write_text("project_state.md", "# Project State\n");

    let first = apply_state_update_for_slice(ApplyStateUpdateOptions {
        workspace_root: workspace.root.clone(),
        queue_path: workspace.root.join("slices/queue.json"),
        slice_id: "L1-orders-001".to_string(),
        author_output_path: None,
    })
    .expect("first state update application should succeed");

    assert!(!first.already_present);

    let second = apply_state_update_for_slice(ApplyStateUpdateOptions {
        workspace_root: workspace.root.clone(),
        queue_path: workspace.root.join("slices/queue.json"),
        slice_id: "L1-orders-001".to_string(),
        author_output_path: None,
    })
    .expect("second state update application should be idempotent");

    assert!(second.already_present);

    let project_state = workspace.read_text("project_state.md");
    assert_eq!(
        project_state
            .matches("### L1-orders-001 — 2026-04-23")
            .count(),
        1
    );
}

#[test]
fn parse_state_update_marker_mismatch_includes_worked_example() {
    let output = "### 🛠️ Execution: L1-orders-001\n#### State Update\n```\nrandom narrative without a marker\n```\n";

    let err = parse_state_update(output, "L1-orders-001")
        .expect_err("missing marker should fail")
        .to_string();

    assert!(err.contains("must start with a slice marker"));
    assert!(err.contains("Expected format"));
    assert!(err.contains("### L1-orders-001 — <YYYY-MM-DD>"));
}

#[test]
fn parse_state_update_detects_pre_fence_marker() {
    let output =
        "#### State Update\n### L1-orders-001 — 2026-04-23\n```\njust some notes go here\n```\n";

    let err = parse_state_update(output, "L1-orders-001")
        .expect_err("pre-fence marker should fail")
        .to_string();

    assert!(err.contains("BEFORE the fenced block"));
    assert!(err.contains("Move the marker INSIDE"));
}

#[test]
fn parse_state_update_detects_diff_prefix_marker() {
    let output =
        "#### State Update\n```\n+ context line\n+ ### L1-orders-001 — 2026-04-23\n+ notes\n```\n";

    let err = parse_state_update(output, "L1-orders-001")
        .expect_err("diff-prefixed marker should fail")
        .to_string();

    assert!(err.contains("unified-diff fence"));
    assert!(err.contains("Drop the `+`/`-`/`@@` prefixes"));
}

#[test]
fn parse_state_update_detects_marker_buried_after_narrative() {
    let output =
        "#### State Update\n```\nSome lead-in narrative.\n### L1-orders-001 — 2026-04-23\n```\n";

    let err = parse_state_update(output, "L1-orders-001")
        .expect_err("buried marker should fail")
        .to_string();

    assert!(err.contains("It must be the FIRST non-blank line"));
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

    fn write_text(&self, relative_path: &str, body: &str) {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture write parent should exist");
        }

        fs::write(path, body).expect("fixture text should write");
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
            "mutagen-harness-{name}-state-update-{}-{nanos}-{attempt}",
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
