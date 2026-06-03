use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::notifications::{
    NotificationEvent, StopCondition, persona_drift_notification, slice_blocker_notification,
    structural_fail_notification,
};
use crate::queue::Slice;
use crate::shell::bash_command;
use crate::state_update::parse_state_update;
use crate::validation::load_queue_file;

// We used to length-gate the drift class with a 600-char floor. The
// 2026-05-12 L4-Workflow-001 escalation killed that floor — an author file
// grew past it with zero canonical headings and dumped ten contract-violation
// findings on the operator. Zero canonical headings is drift regardless of
// length; the historical threshold lives on only in the finding message for
// forensic comparison.
pub(crate) const PERSONA_DRIFT_CHAR_THRESHOLD: usize = 600;

#[derive(Debug, Clone)]
pub struct StructuralCheckOptions {
    pub slice_id: String,
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub author_output_dir: PathBuf,
    pub loc_script_path: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StructuralVerdict {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StructuralSeverity {
    Fail,
    Warn,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructuralFinding {
    pub check: String,
    pub severity: StructuralSeverity,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StructuralCheckReport {
    pub verdict: StructuralVerdict,
    pub findings: Vec<StructuralFinding>,
    pub loc: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_condition: Option<StopCondition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notifications: Vec<NotificationEvent>,
}

pub fn structural_check(options: StructuralCheckOptions) -> StructuralCheckReport {
    if options.slice_id.trim().is_empty() {
        return fail_report("args", "missing slice_id");
    }

    let queue = match load_queue_file(&options.queue_path) {
        Ok(queue) => queue,
        Err(_) => {
            return fail_report(
                "queue",
                &format!("{} not readable", display_path(&options.queue_path)),
            );
        }
    };

    let Some(slice) = queue
        .slices
        .iter()
        .find(|slice| slice.id == options.slice_id)
    else {
        return fail_report("queue", &format!("slice {} not found", options.slice_id));
    };

    let Some(required_sections) = required_output_contract_for_author(&slice.author_agent) else {
        return fail_report(
            "author_agent",
            &format!("unknown author_agent {}", slice.author_agent),
        );
    };

    let author_output_path = resolve_path(&options.workspace_root, &options.author_output_dir)
        .join(format!("{}.md", options.slice_id));

    let author_output = match fs::read_to_string(&author_output_path) {
        Ok(author_output) => author_output,
        Err(_) => {
            return StructuralCheckReport {
                verdict: StructuralVerdict::Fail,
                findings: vec![finding(
                    "author_output",
                    StructuralSeverity::Fail,
                    format!(
                        "author output not found at {} — orchestrator must write it before calling this script",
                        display_path(&author_output_path)
                    ),
                )],
                loc: empty_object(),
                stop_condition: Some(StopCondition::StructuralFailure),
                notifications: vec![structural_fail_notification(
                    &slice.id,
                    &slice.title,
                    "author output not found",
                )],
            };
        }
    };

    let mut findings = Vec::new();

    // Slice Blocker gate (per 2026-05-06 L1-Infra-005 postmortem). An author
    // that finds its slice unexecutable inside its declared write_set can
    // surface a `## Slice Blocker` heading naming the reason. We recognize
    // three tokens — `scope_amendment_request`, `predecessor_repair_required`,
    // `contradictory_traces` — and route to a typed escalation instead of
    // dumping a wall of "missing required heading" findings on the operator.
    // The block has to be a real markdown heading anchor; substring-mentions
    // of "slice blocker" elsewhere in the body don't count.
    if let Some(blocker) = parse_slice_blocker(&author_output) {
        findings.push(finding(
            "slice_blocker",
            StructuralSeverity::Fail,
            format!(
                "author '{}' raised a slice blocker (reason: {}). Body: {}",
                slice.author_agent, blocker.reason_token, blocker.body_excerpt
            ),
        ));
        return StructuralCheckReport {
            verdict: StructuralVerdict::Fail,
            findings,
            loc: collect_loc(&options),
            stop_condition: Some(StopCondition::SliceBlocker),
            notifications: vec![slice_blocker_notification(
                &slice.id,
                &slice.title,
                &blocker.reason_token,
                &blocker.body_excerpt,
            )],
        };
    }

    // Persona-drift gate: zero canonical headings is drift, full stop. We
    // used to gate on `sections_present == 0 AND trimmed_len < 600` but the
    // 2026-05-12 L4-Workflow-001 escalation proved the length floor was a
    // false guard — a 1k+ char author dump with zero canonical headings
    // generated ten contract-violation findings that all said the same
    // thing. Persona-drift is one finding; structural-failure is many.
    let sections_present = required_sections
        .iter()
        .filter(|section| required_section_present(&author_output, section))
        .count();
    let trimmed_len = author_output.trim().chars().count();
    if sections_present == 0 {
        findings.push(finding(
            "persona_drift",
            StructuralSeverity::Fail,
            format!(
                "author '{}' emitted {} chars with zero required sections (well-formed deliverables clear ~{} chars); treating as persona-drift, not contract-violation. Dispatch payload preserved at .mutagen/state/dispatch/{}/author-initial.md for forensics.",
                slice.author_agent, trimmed_len, PERSONA_DRIFT_CHAR_THRESHOLD, slice.id
            ),
        ));
        return StructuralCheckReport {
            verdict: StructuralVerdict::Fail,
            findings,
            loc: collect_loc(&options),
            stop_condition: Some(StopCondition::PersonaDrift),
            notifications: vec![persona_drift_notification(
                &slice.id,
                &slice.title,
                &slice.author_agent,
                trimmed_len,
            )],
        };
    }

    for section in required_sections {
        if !required_section_present(&author_output, section) {
            findings.push(finding(
                "required_section",
                StructuralSeverity::Fail,
                format!("missing required markdown heading: {}", section.marker),
            ));
        }
    }

    for cited_id in cited_ids_for_structural_check(slice) {
        if !citation_present(&author_output, cited_id) {
            findings.push(finding(
                "traces_to_drift",
                StructuralSeverity::Fail,
                format!("cited ID {cited_id} does not appear in author output"),
            ));
        }
    }

    if let Err(error) = parse_state_update(&author_output, &options.slice_id) {
        findings.push(finding(
            "state_block",
            StructuralSeverity::Fail,
            error.to_string(),
        ));
    }

    let loc = collect_loc(&options);
    let over_target_pct = loc
        .get("over_target_pct")
        .and_then(Value::as_u64)
        .unwrap_or_default();

    if over_target_pct > 120 {
        findings.push(finding(
            "loc_overrun",
            StructuralSeverity::Fail,
            format!(
                "net LOC is {}% of target {} — exceeds 120% hard gate",
                over_target_pct, slice.target_loc
            ),
        ));
    }

    let verdict = if findings
        .iter()
        .any(|finding| finding.severity == StructuralSeverity::Fail)
    {
        StructuralVerdict::Fail
    } else {
        StructuralVerdict::Pass
    };
    let stop_condition =
        (verdict == StructuralVerdict::Fail).then_some(StopCondition::StructuralFailure);
    let notifications = if verdict == StructuralVerdict::Fail {
        vec![structural_fail_notification(
            &slice.id,
            &slice.title,
            findings
                .first()
                .map(|finding| finding.detail.as_str())
                .unwrap_or("structural check failed"),
        )]
    } else {
        Vec::new()
    };

    StructuralCheckReport {
        verdict,
        findings,
        loc,
        stop_condition,
        notifications,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequiredAuthorSection {
    pub marker: &'static str,
    pub kind: RequiredSectionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredSectionKind {
    MarkdownHeading,
}

// Single source of truth for the structural author-output contract. The
// dispatch prompt renderer uses this same table to avoid prompt/runtime drift.
pub fn required_output_contract_for_author(
    author_agent: &str,
) -> Option<&'static [RequiredAuthorSection]> {
    match author_agent {
        "Bebop" => Some(&BEBOP_CONTRACT),
        "Baxter" => Some(&BAXTER_CONTRACT),
        "Chaplin" => Some(&CHAPLIN_CONTRACT),
        "Metalhead" => Some(&METALHEAD_CONTRACT),
        "Splinter" => Some(&SPLINTER_CONTRACT),
        "Tatsu" => Some(&TATSU_CONTRACT),
        "Krang" => Some(&KRANG_CONTRACT),
        _ => None,
    }
}

const fn section(marker: &'static str) -> RequiredAuthorSection {
    RequiredAuthorSection {
        marker,
        kind: RequiredSectionKind::MarkdownHeading,
    }
}

const BEBOP_CONTRACT: [RequiredAuthorSection; 6] = [
    section("🛠️ Execution:"),
    section("Intake Report"),
    section("Code Artifacts"),
    section("ISC Upholding Map"),
    section("Verification Artifacts"),
    section("State Update"),
];

const BAXTER_CONTRACT: [RequiredAuthorSection; 7] = [
    section("🔬 Execution:"),
    section("Intake Report"),
    section("Algorithmic Proof"),
    section("Code Artifacts"),
    section("ISC Upholding Map"),
    section("Verification Artifacts"),
    section("State Update"),
];

const CHAPLIN_CONTRACT: [RequiredAuthorSection; 7] = [
    section("💽 Execution:"),
    section("Intake Report"),
    section("Data Model Analysis"),
    section("Code Artifacts"),
    section("ISC Upholding Map"),
    section("Verification Artifacts"),
    section("State Update"),
];

const METALHEAD_CONTRACT: [RequiredAuthorSection; 7] = [
    section("📡 Execution:"),
    section("Intake Report"),
    section("Observability Plan"),
    section("Code Artifacts"),
    section("ISC Upholding Map"),
    section("Verification Artifacts"),
    section("State Update"),
];

const SPLINTER_CONTRACT: [RequiredAuthorSection; 7] = [
    section("🐀 Execution:"),
    section("Intake Report"),
    section("Documentation Brief"),
    section("Drafted Artefacts"),
    section("Cross-check Notes"),
    section("Verification Artifacts"),
    section("State Update"),
];

const TATSU_CONTRACT: [RequiredAuthorSection; 7] = [
    section("🥷 Execution:"),
    section("Intake Report"),
    section("Threat Model"),
    section("Code Artifacts"),
    section("ISC Upholding Map"),
    section("Verification Artifacts"),
    section("State Update"),
];

const KRANG_CONTRACT: [RequiredAuthorSection; 6] = [
    section("🧠 Execution:"),
    section("Intake Report"),
    section("Infrastructure Artifacts"),
    section("ISC Enforcement Map"),
    section("Verification Artifacts"),
    section("State Update"),
];

// Required-section recognition. A line counts as a heading match for
// `section.marker` if any of the following hold:
//   (a) `## <marker>` — canonical h2.
//   (b) `### <marker>` (or deeper) — depth-tolerant, preserved from the prior
//       behavior where any `#`-prefixed heading whose title starts with the
//       marker satisfied the check.
//   (c) `**<marker>**` on its own line — the bold-line equivalent. Match is
//       exact-on-the-marker: trailing whitespace before/after the closing
//       `**` disqualifies, and embedded bold inside prose (`**Intake Report**
//       is great`) does not count. The 2026-05-12 L4-Session-001 escalation
//       drove this loosening — Bebop emitted complete contract-faithful
//       content under bold markers and the parser fired six false positives.
// The closed set of refusal reasons authors can stamp on a Slice Blocker.
// Anything else still escalates, but as `unknown_reason` — we keep the door
// open so an author can name a novel blocker class without us silently
// downgrading it to drift.
const SLICE_BLOCKER_REASONS: &[&str] = &[
    "scope_amendment_request",
    "predecessor_repair_required",
    "contradictory_traces",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SliceBlocker {
    pub reason_token: String,
    pub body_excerpt: String,
}

pub(crate) fn parse_slice_blocker(author_output: &str) -> Option<SliceBlocker> {
    let mut body_lines: Vec<&str> = Vec::new();
    let mut found_heading = false;

    for line in author_output.lines() {
        if !found_heading {
            if is_slice_blocker_heading(line) {
                found_heading = true;
            }
            continue;
        }

        // Stop at the next markdown heading — body is everything between the
        // Slice Blocker heading and the next `#`-prefixed line.
        if line.trim_start().starts_with('#') {
            break;
        }
        body_lines.push(line);
    }

    if !found_heading {
        return None;
    }

    let body = body_lines.join("\n");
    let body_trimmed = body.trim();

    let reason_token = SLICE_BLOCKER_REASONS
        .iter()
        .find(|token| body_trimmed.contains(*token))
        .map(|token| (*token).to_string())
        .unwrap_or_else(|| "unknown_reason".to_string());

    let excerpt_chars: String = body_trimmed.chars().take(240).collect();
    let body_excerpt = if body_trimmed.chars().count() > 240 {
        format!("{excerpt_chars}…")
    } else {
        excerpt_chars
    };

    Some(SliceBlocker {
        reason_token,
        body_excerpt,
    })
}

fn is_slice_blocker_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    let body = rest.trim_start_matches('#').trim();
    body.eq_ignore_ascii_case("slice blocker")
}

fn required_section_present(author_output: &str, section: &RequiredAuthorSection) -> bool {
    match section.kind {
        RequiredSectionKind::MarkdownHeading => author_output
            .lines()
            .any(|line| line_matches_section_marker(line, section.marker)),
    }
}

fn line_matches_section_marker(line: &str, marker: &str) -> bool {
    if let Some(title) = markdown_heading_title(line)
        && title.starts_with(marker)
    {
        return true;
    }

    bold_line_marker(line)
        .map(|body| body == marker)
        .unwrap_or(false)
}

fn markdown_heading_title(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if level == 0 {
        return None;
    }

    let title = trimmed[level..].trim_start();
    (!title.is_empty()).then_some(title)
}

// Pull the inner text of a line that is exactly `**...**` and nothing else.
// Whitespace at the line edges is fine (leading/trailing). Whitespace adjacent
// to the `**` markers is NOT — `** Intake Report **` returns None. Anything
// after the closing `**` (`**Intake Report** is great`) also returns None.
fn bold_line_marker(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let stripped = trimmed.strip_prefix("**")?.strip_suffix("**")?;
    if stripped.is_empty() {
        return None;
    }
    if stripped.starts_with(char::is_whitespace) || stripped.ends_with(char::is_whitespace) {
        return None;
    }
    Some(stripped)
}

fn cited_ids_for_structural_check(slice: &Slice) -> impl Iterator<Item = &str> {
    slice
        .traces_to
        .prd
        .iter()
        .chain(slice.traces_to.adr.iter())
        .chain(slice.traces_to.isc.iter())
        .chain(slice.traces_to.dsd.iter())
        .map(String::as_str)
}

// Citation-presence scan with separator awareness.
//
// The 2026-05-10 traces_to_drift postmortem caught Chaplin writing DSD-622
// as part of the joined form `DSD-621/622`. A plain str::contains never saw
// it. We keep the literal substring scan as the fast path (covers bracketed
// forms like `[FR-001]` and any author that wrote the ID verbatim) and add a
// separator-aware walk for bare `PREFIX-NUMBER` citations. A bare cited id
// `DSD-622` matches author text that says any of:
//   - `DSD-622` directly (literal substring),
//   - `DSD-621/622` or `DSD-621, 622` or `DSD-620, 621, 622` (joined run),
//   - the same with `&` joining numbers.
//
// We do NOT accept ranges like `DSD-620-622` — that form is ambiguous (range
// vs typo) and the postmortem only demanded joined-pair tolerance. We also
// require a word-boundary before the `<PREFIX>-` to avoid matching `XDSD-622`
// when DSD-622 is cited.
fn citation_present(author_output: &str, cited_id: &str) -> bool {
    if author_output.contains(cited_id) {
        return true;
    }
    let Some((prefix, target_num)) = split_bare_citation(cited_id) else {
        return false;
    };
    scan_joined_citation(author_output, prefix, target_num)
}

fn split_bare_citation(id: &str) -> Option<(&str, &str)> {
    let dash = id.find('-')?;
    let prefix = &id[..dash];
    let number = &id[dash + 1..];
    if prefix.is_empty() || number.is_empty() {
        return None;
    }
    if !prefix.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    if !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((prefix, number))
}

fn scan_joined_citation(text: &str, prefix: &str, target_num: &str) -> bool {
    let bytes = text.as_bytes();
    let needle = format!("{prefix}-");
    let needle_bytes = needle.as_bytes();

    let mut cursor = 0;
    while cursor + needle_bytes.len() <= bytes.len() {
        let Some(rel) = bytes[cursor..]
            .windows(needle_bytes.len())
            .position(|window| window == needle_bytes)
        else {
            return false;
        };
        let pos = cursor + rel;

        let boundary_ok = pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric();
        if !boundary_ok {
            cursor = pos + 1;
            continue;
        }

        if joined_number_run_contains(bytes, pos + needle_bytes.len(), target_num) {
            return true;
        }
        cursor = pos + needle_bytes.len();
    }
    false
}

// Starting just past the `PREFIX-`, read a comma/slash/ampersand-joined run
// of decimal numbers. Return true if any number in the run equals target_num.
// Whitespace between joiner and the next number is tolerated. The very first
// number group is mandatory; the rest are optional, repeating.
fn joined_number_run_contains(bytes: &[u8], mut idx: usize, target_num: &str) -> bool {
    let target_bytes = target_num.as_bytes();
    loop {
        let num_start = idx;
        while idx < bytes.len() && bytes[idx].is_ascii_digit() {
            idx += 1;
        }
        if idx == num_start {
            return false;
        }
        if &bytes[num_start..idx] == target_bytes {
            return true;
        }
        // Allow optional whitespace before the joiner too — e.g. `FR-13 & 14`.
        let mut probe = idx;
        while probe < bytes.len() && (bytes[probe] == b' ' || bytes[probe] == b'\t') {
            probe += 1;
        }
        if probe >= bytes.len() {
            return false;
        }
        match bytes[probe] {
            b'/' | b',' | b'&' => {
                idx = probe + 1;
                while idx < bytes.len() && (bytes[idx] == b' ' || bytes[idx] == b'\t') {
                    idx += 1;
                }
            }
            _ => return false,
        }
    }
}

fn collect_loc(options: &StructuralCheckOptions) -> Value {
    let loc_script_path = resolve_path(&options.workspace_root, &options.loc_script_path);
    if !loc_script_path.exists() {
        return empty_object();
    }

    let command_path = loc_script_path
        .strip_prefix(&options.workspace_root)
        .map(PathBuf::from)
        .unwrap_or(loc_script_path);

    let output = match bash_command()
        .arg(&command_path)
        .arg(&options.slice_id)
        .current_dir(&options.workspace_root)
        .output()
    {
        Ok(output) => output,
        Err(_) => return empty_object(),
    };

    if !output.status.success() {
        return empty_object();
    }

    serde_json::from_slice::<Value>(&output.stdout)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(empty_object)
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn fail_report(check: &str, detail: &str) -> StructuralCheckReport {
    StructuralCheckReport {
        verdict: StructuralVerdict::Fail,
        findings: vec![finding(check, StructuralSeverity::Fail, detail.to_string())],
        loc: empty_object(),
        stop_condition: None,
        notifications: Vec::new(),
    }
}

fn finding(check: &str, severity: StructuralSeverity, detail: String) -> StructuralFinding {
    StructuralFinding {
        check: check.to_string(),
        severity,
        detail,
    }
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod citation_present_tests {
    use super::citation_present;

    #[test]
    fn literal_substring_still_wins() {
        let body = "Cites DSD-622 directly somewhere in the body.";
        assert!(citation_present(body, "DSD-622"));
    }

    #[test]
    fn slash_joined_pair_counts_for_both_sides() {
        let body = "Implements DSD-621/622 together.";
        assert!(citation_present(body, "DSD-621"));
        assert!(citation_present(body, "DSD-622"));
    }

    #[test]
    fn comma_joined_run_counts_for_every_member() {
        let body = "Upholds DSD-620, 621, 622 across the writer.";
        assert!(citation_present(body, "DSD-620"));
        assert!(citation_present(body, "DSD-621"));
        assert!(citation_present(body, "DSD-622"));
    }

    #[test]
    fn ampersand_joiner_supported() {
        let body = "FR-13 & 14 are both covered.";
        assert!(citation_present(body, "FR-13"));
        assert!(citation_present(body, "FR-14"));
    }

    #[test]
    fn unrelated_id_in_joined_run_does_not_satisfy() {
        let body = "Implements DSD-621/622 together.";
        assert!(!citation_present(body, "DSD-700"));
    }

    #[test]
    fn bracketed_form_kept_strict_via_fast_path() {
        let body = "Upholds [FR-001] in the aggregate.";
        assert!(citation_present(body, "[FR-001]"));
        // The bare-form fallback should not fire for bracketed IDs.
        assert!(!citation_present("Upholds [FR-001/002].", "[FR-002]"));
    }

    #[test]
    fn missing_citation_returns_false() {
        let body = "Talks about FR-13 only.";
        assert!(!citation_present(body, "FR-14"));
        assert!(!citation_present(body, "DSD-622"));
    }

    #[test]
    fn range_form_with_hyphen_not_accepted() {
        // `DSD-620-622` is ambiguous (range vs typo). Postmortem only demanded
        // joined-pair tolerance; we keep ranges strict.
        let body = "Spans DSD-620-622 historically.";
        assert!(citation_present(body, "DSD-620"));
        assert!(!citation_present(body, "DSD-622"));
    }

    #[test]
    fn whitespace_after_joiner_tolerated() {
        let body = "Pairs DSD-621,   622 in the manifest.";
        assert!(citation_present(body, "DSD-622"));
    }

    // Known limitation: the literal-substring fast path still matches across
    // numeric over-runs — `DSD-6224` satisfies `DSD-622`, `XDSD-622` satisfies
    // `DSD-622`. The 2026-05-10 postmortem only required joined-pair tolerance
    // and `str::contains` predates this helper; tightening the fast path to
    // be word-boundary aware is a separate follow-up.
}
