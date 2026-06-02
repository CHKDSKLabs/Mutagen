use mutagen_core::queue_readiness::{QUEUE_CONTRACT_HASH_BASIS, queue_contract_hash};
use mutagen_core::validation::validate_queue_file;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub fn queue_validation_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".mutagen/state/queue-validation.json")
}

pub fn write_queue_validation(workspace_root: &Path) {
    let queue_path = workspace_root.join("slices/queue.json");
    let validation_path = queue_validation_path(workspace_root);
    let report = validate_queue_file(&queue_path).expect("queue validation should run");
    let mut report: Value = serde_json::to_value(report).expect("queue report should serialize");
    let hash = queue_contract_hash(&queue_path).expect("queue hash should compute");

    report["queue"] = serde_json::json!(queue_path.to_string_lossy());
    report["queue_contract_hash"] = serde_json::json!(hash);
    report["queue_contract_hash_basis"] = serde_json::json!(QUEUE_CONTRACT_HASH_BASIS);
    report["queue_contract_hash_algorithm"] = serde_json::json!("sha1");

    if let Some(parent) = validation_path.parent() {
        fs::create_dir_all(parent).expect("queue validation parent should exist");
    }

    let body = serde_json::to_string_pretty(&report).expect("queue report should serialize");
    fs::write(validation_path, format!("{body}\n")).expect("queue validation should write");
}
