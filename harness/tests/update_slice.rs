use mutagen_harness::queue::{
    BishopVerdict, KaraiStructuralVerdict, SliceStatus, TigerClawVerdict,
};
use mutagen_harness::queue_update::{UpdateSliceOptions, update_slice};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn update_slice_records_runtime_verdicts_and_escalation() {
    let workspace = FixtureWorkspace::copy("basic_ready");

    let result = update_slice(UpdateSliceOptions {
        queue_path: workspace.root.join("slices/queue.json"),
        slice_id: "L1-orders-001".to_string(),
        status: Some(SliceStatus::Escalated),
        attempts: Some(2),
        micro_corrections_used: Some(1),
        karai_structural: Some(KaraiStructuralVerdict::Fail),
        bishop: Some(BishopVerdict::Skip),
        tiger_claw: Some(TigerClawVerdict::Defect),
        micro_correction: Some(false),
        completed_at: None,
        clear_completed_at: false,
        escalation_reason: Some("Structural check halted the slice.".to_string()),
        clear_escalation_reason: false,
    })
    .expect("update-slice should succeed");

    assert_eq!(result.status, SliceStatus::Escalated);
    assert_eq!(result.attempts, 2);
    assert_eq!(result.micro_corrections_used, 1);
    assert_eq!(
        result.verdicts.karai_structural,
        Some(KaraiStructuralVerdict::Fail)
    );
    assert_eq!(result.verdicts.bishop, Some(BishopVerdict::Skip));
    assert_eq!(result.verdicts.tiger_claw, Some(TigerClawVerdict::Defect));
    assert_eq!(result.verdicts.micro_correction, Some(false));
    assert_eq!(result.verdicts.micro_corrections_used, Some(1));
    assert_eq!(
        result.escalation_reason.as_deref(),
        Some("Structural check halted the slice.")
    );

    let queue = workspace.read_json("slices/queue.json");
    let slice = &queue["slices"][0];
    assert_eq!(slice["status"], "escalated");
    assert_eq!(slice["attempts"], 2);
    assert_eq!(slice["micro_corrections_used"], 1);
    assert_eq!(slice["verdicts"]["karai_structural"], "fail");
    assert_eq!(slice["verdicts"]["bishop"], "skip");
    assert_eq!(slice["verdicts"]["tiger_claw"], "defect");
    assert_eq!(slice["verdicts"]["micro_correction"], false);
    assert_eq!(slice["verdicts"]["micro_corrections_used"], 1);
    assert_eq!(
        slice["escalation_reason"],
        "Structural check halted the slice."
    );
}

#[test]
fn update_slice_records_completion_fields() {
    let workspace = FixtureWorkspace::copy("basic_ready");

    let result = update_slice(UpdateSliceOptions {
        queue_path: workspace.root.join("slices/queue.json"),
        slice_id: "L1-orders-001".to_string(),
        status: Some(SliceStatus::Completed),
        attempts: None,
        micro_corrections_used: None,
        karai_structural: Some(KaraiStructuralVerdict::Pass),
        bishop: Some(BishopVerdict::Skip),
        tiger_claw: Some(TigerClawVerdict::Clean),
        micro_correction: Some(true),
        completed_at: Some("2026-04-22T18:00:00Z".to_string()),
        clear_completed_at: false,
        escalation_reason: None,
        clear_escalation_reason: false,
    })
    .expect("update-slice should succeed");

    assert_eq!(result.status, SliceStatus::Completed);
    assert_eq!(result.completed_at.as_deref(), Some("2026-04-22T18:00:00Z"));
    assert_eq!(
        result.verdicts.karai_structural,
        Some(KaraiStructuralVerdict::Pass)
    );
    assert_eq!(result.verdicts.bishop, Some(BishopVerdict::Skip));
    assert_eq!(result.verdicts.tiger_claw, Some(TigerClawVerdict::Clean));
    assert_eq!(result.verdicts.micro_correction, Some(true));

    let queue = workspace.read_json("slices/queue.json");
    let slice = &queue["slices"][0];
    assert_eq!(slice["status"], "completed");
    assert_eq!(slice["completed_at"], "2026-04-22T18:00:00Z");
    assert_eq!(slice["verdicts"]["karai_structural"], "pass");
    assert_eq!(slice["verdicts"]["bishop"], "skip");
    assert_eq!(slice["verdicts"]["tiger_claw"], "clean");
    assert_eq!(slice["verdicts"]["micro_correction"], true);
}

#[test]
fn update_slice_can_clear_runtime_fields() {
    let workspace = FixtureWorkspace::copy("basic_ready");

    let mut queue = workspace.read_json("slices/queue.json");
    queue["slices"][0]["completed_at"] = Value::String("2026-04-22T18:00:00Z".to_string());
    queue["slices"][0]["escalation_reason"] = Value::String("Something exploded.".to_string());
    workspace.write_json("slices/queue.json", &queue);

    let result = update_slice(UpdateSliceOptions {
        queue_path: workspace.root.join("slices/queue.json"),
        slice_id: "L1-orders-001".to_string(),
        status: Some(SliceStatus::Pending),
        attempts: None,
        micro_corrections_used: None,
        karai_structural: None,
        bishop: None,
        tiger_claw: None,
        micro_correction: None,
        completed_at: None,
        clear_completed_at: true,
        escalation_reason: None,
        clear_escalation_reason: true,
    })
    .expect("update-slice should succeed");

    assert!(result.completed_at.is_none());
    assert!(result.escalation_reason.is_none());

    let queue = workspace.read_json("slices/queue.json");
    let slice = &queue["slices"][0];
    assert!(slice.get("completed_at").is_none());
    assert!(slice.get("escalation_reason").is_none());
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

    fn read_json(&self, relative_path: &str) -> Value {
        let raw =
            fs::read_to_string(self.root.join(relative_path)).expect("fixture JSON should read");
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
            "mutagen-harness-{name}-update-slice-{}-{nanos}-{attempt}",
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
