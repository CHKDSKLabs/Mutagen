use mutagen_core::queue::TigerClawVerdict;
use mutagen_core::review_record::parse_tiger_claw_verdict;

#[test]
fn accepts_h2_verdict_heading() {
    let report = "## Verdict\n\n🟢 Clean\n";
    assert_eq!(
        parse_tiger_claw_verdict(report),
        Some(TigerClawVerdict::Clean)
    );
}

#[test]
fn accepts_h3_verdict_heading_defect() {
    let report = "### Verdict\n\n🔴 Defect found in input validation\n";
    assert_eq!(
        parse_tiger_claw_verdict(report),
        Some(TigerClawVerdict::Defect)
    );
}

#[test]
fn accepts_h4_verdict_heading_gap() {
    let report = "#### Verdict\n\n🟡 Gap — missing coverage on the empty-payload branch\n";
    assert_eq!(
        parse_tiger_claw_verdict(report),
        Some(TigerClawVerdict::Gap)
    );
}

#[test]
fn accepts_label_only_path_without_heading() {
    let report = "Tiger Claw drove the report into a wall.\n\n**Verdict:** Clean\n";
    assert_eq!(
        parse_tiger_claw_verdict(report),
        Some(TigerClawVerdict::Clean)
    );
}

#[test]
fn classifies_qa_defect_token() {
    let report = "## Verdict\n\nqa-defect: idempotency key absent on retry\n";
    assert_eq!(
        parse_tiger_claw_verdict(report),
        Some(TigerClawVerdict::Defect)
    );
}

#[test]
fn rejects_unrelated_headings_with_verdict_word() {
    // "## Bishop Verdict" or "## Verdict Notes" mention the word but are not the
    // canonical section anchor — they must not flip the parser into section mode.
    let report = "## Bishop Verdict\n\nClean here is unrelated.\n\n## Findings\n\nnothing\n";
    assert_eq!(parse_tiger_claw_verdict(report), None);
}

#[test]
fn malformed_report_returns_none() {
    let report = "Tiger Claw walked off the job. No verdict to be found.\n";
    assert_eq!(parse_tiger_claw_verdict(report), None);
}
