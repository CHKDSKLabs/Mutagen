use mutagen_core::state_update::parse_state_update;

const SLICE: &str = "L4-Session-001";

fn run(body_inside_fence: &str) -> anyhow::Result<mutagen_core::state_update::ParsedStateUpdate> {
    let output = format!(
        "### 🛠️ Execution: {SLICE}\n#### State Update\n```\n{body}\n```\n",
        body = body_inside_fence
    );
    parse_state_update(&output, SLICE)
}

#[test]
fn marker_on_first_line_accepts() {
    let parsed = run(&format!(
        "### {SLICE} — 2026-05-12\n\nSessions opened on the long socket.\n"
    ))
    .expect("canonical marker placement should parse");

    assert_eq!(parsed.marker, format!("### {SLICE} — 2026-05-12"));
    assert!(parsed.body.starts_with(&format!("### {SLICE}")));
    assert!(parsed.body.contains("Sessions opened on the long socket."));
}

#[test]
fn marker_after_one_metadata_line_accepts() {
    // The 2026-05-12 L4-Session-001 incident: author led with `target: …` then the marker.
    let parsed = run(&format!(
        "target: project_state.md § Sessions\n### {SLICE} — 2026-05-12\n\nSocket loop restarts now drain in flight.\n"
    ))
    .expect("one metadata line before the marker should be tolerated");

    assert_eq!(parsed.marker, format!("### {SLICE} — 2026-05-12"));
    // Consumed metadata line should NOT leak into the persisted body.
    assert!(
        !parsed.body.contains("target:"),
        "metadata prefix line should be stripped, got body: {}",
        parsed.body
    );
    assert!(parsed.body.starts_with(&format!("### {SLICE}")));
    assert!(
        parsed
            .body
            .contains("Socket loop restarts now drain in flight.")
    );
}

#[test]
fn marker_after_two_non_metadata_lines_rejects() {
    let err = run(&format!(
        "some narrative\nmore narrative\n### {SLICE} — 2026-05-12\nnotes\n"
    ))
    .expect_err("marker buried beyond the metadata window must reject");
    let msg = err.to_string();

    assert!(
        msg.contains("must start with a slice marker"),
        "error should explain the marker rule, got: {msg}"
    );
    assert!(
        msg.contains("some narrative"),
        "error should name the unrecognized leading line verbatim, got: {msg}"
    );
}

#[test]
fn marker_after_one_metadata_line_with_colon_in_value_accepts() {
    // Only the leading `key:` is consumed; subsequent colons in the value are
    // just data and must not derail the relaxation.
    let parsed = run(&format!(
        "target: project_state.md § Sessions: long-socket lane\n### {SLICE} — 2026-05-12\n\nNo regressions reported.\n"
    ))
    .expect("colon-in-value metadata should still parse");

    assert_eq!(parsed.marker, format!("### {SLICE} — 2026-05-12"));
    assert!(
        !parsed.body.contains("target:"),
        "the metadata line (with a colon-in-value) should not leak into body: {}",
        parsed.body
    );
    assert!(
        !parsed.body.contains("long-socket lane"),
        "no fragment of the metadata line should survive in body: {}",
        parsed.body
    );
    assert!(parsed.body.contains("No regressions reported."));
}

#[test]
fn two_metadata_lines_before_marker_reject() {
    // The relaxation is exactly one line wide. A second metadata pair means
    // the marker is in position three and the parser stays strict.
    let err = run(&format!(
        "target: project_state.md § Sessions\nauthor: bebop\n### {SLICE} — 2026-05-12\nnotes\n"
    ))
    .expect_err("two metadata lines before the marker must reject");
    let msg = err.to_string();

    assert!(
        msg.contains("must start with a slice marker"),
        "error should explain the marker rule, got: {msg}"
    );
    assert!(
        msg.contains("author: bebop"),
        "error should name the second leading line verbatim, got: {msg}"
    );
}

#[test]
fn duplicate_marker_in_body_rejects() {
    // Marker uniqueness: a second `### slice — date` line is almost always a
    // copy-paste accident and would silently collapse on the dedup path.
    let err = run(&format!(
        "### {SLICE} — 2026-05-12\nnotes for the morning incident\n### {SLICE} — 2026-05-12\nnotes for the evening incident\n"
    ))
    .expect_err("two slice markers in one block must reject");
    assert!(
        err.to_string().contains("more than one slice marker"),
        "error should call out the duplicate marker, got: {err}"
    );
}
