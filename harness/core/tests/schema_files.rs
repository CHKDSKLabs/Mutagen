use serde_json::Value;
use std::fs;
use std::path::PathBuf;

#[test]
fn harness_schema_files_parse_as_json() {
    let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("mutagen-core lives under harness/")
        .join("schemas");
    let mut schema_paths = fs::read_dir(&schema_dir)
        .expect("schema directory should be readable")
        .map(|entry| entry.expect("schema entry should be readable").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();

    schema_paths.sort();
    assert!(
        !schema_paths.is_empty(),
        "schema directory should contain JSON schemas"
    );

    for path in schema_paths {
        let raw = fs::read_to_string(&path).expect("schema file should be readable");
        let schema: Value = serde_json::from_str(&raw).expect("schema file should parse as JSON");

        assert_eq!(
            schema.get("$schema").and_then(Value::as_str),
            Some("https://json-schema.org/draft/2020-12/schema"),
            "{} should declare the JSON Schema draft",
            path.display()
        );
        assert!(
            schema.get("$id").and_then(Value::as_str).is_some(),
            "{} should declare a stable schema id",
            path.display()
        );
        assert!(
            schema.get("title").and_then(Value::as_str).is_some(),
            "{} should declare a human-readable title",
            path.display()
        );
    }
}
