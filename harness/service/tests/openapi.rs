use std::path::PathBuf;

use mutagen_service::openapi::{self, GENERATED_MARKER};
use serde_json::{Value, json};

fn committed_spec_path() -> PathBuf {
    // tests run with CARGO_MANIFEST_DIR = harness/service. Repo root is two up.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .join("..")
        .join("..")
        .join("docs")
        .join("openapi.json")
}

fn normalize(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

#[test]
fn committed_spec_matches_generated() {
    let generated = openapi::spec_json().expect("generate spec");
    let on_disk = std::fs::read(committed_spec_path()).expect("read committed openapi.json");
    assert_eq!(
        normalize(&generated),
        normalize(&on_disk),
        "docs/openapi.json drifted from ApiDoc — run `cargo run -p xtask -- openapi`"
    );
}

#[test]
fn drift_fails_when_handler_changes_unaccompanied() {
    let mut spec = openapi::spec_value().expect("build spec value");
    let obj = spec.as_object_mut().expect("spec is a JSON object");
    let paths = obj
        .entry("paths")
        .or_insert_with(|| Value::Object(Default::default()));
    let paths_obj = paths.as_object_mut().expect("paths is a JSON object");
    paths_obj.insert(
        "/ghost".to_string(),
        json!({
            "get": {
                "summary": "phantom endpoint — should never ship",
                "responses": { "200": { "description": "ok" } }
            }
        }),
    );

    let mutated = openapi::render(&spec).expect("render mutated spec");
    let on_disk = std::fs::read(committed_spec_path()).expect("read committed openapi.json");
    assert_ne!(
        normalize(&mutated),
        normalize(&on_disk),
        "drift gate is asleep: a synthetic /ghost path matched docs/openapi.json"
    );
}

#[test]
fn aggregator_carries_generated_marker() {
    let value = openapi::spec_value().expect("build spec value");
    let marker = value
        .get("$comment")
        .and_then(Value::as_str)
        .expect("$comment marker missing from spec");
    assert_eq!(marker, GENERATED_MARKER);
}

#[test]
fn aggregator_advertises_service_name() {
    let value = openapi::spec_value().expect("build spec value");
    let title = value
        .pointer("/info/title")
        .and_then(Value::as_str)
        .expect("info.title missing");
    assert_eq!(title, "mutagen-service");
}
