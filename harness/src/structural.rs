use serde::Serialize;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::notifications::{NotificationEvent, StopCondition, structural_fail_notification};
use crate::queue::Slice;
use crate::state_update::parse_state_update;
use crate::validation::load_queue_file;

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

    let Some(required_sections) = required_sections_for_author(&slice.author_agent) else {
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

    for section in required_sections {
        if !author_output.contains(section) {
            findings.push(finding(
                "required_section",
                StructuralSeverity::Fail,
                format!("missing required section: {section}"),
            ));
        }
    }

    for cited_id in cited_ids_for_structural_check(slice) {
        if !author_output.contains(cited_id) {
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

// Single source of truth for "what literal markers must appear in this persona's
// author output". Both the structural check and the dispatch prompt-renderer consult
// this -- if they disagree, an author can produce output that the prompt told them
// to write but which the structural check then rejects. So: never duplicate this list.
pub fn required_sections_for_author(author_agent: &str) -> Option<&'static [&'static str]> {
    match author_agent {
        "Bebop" => Some(&[
            "🛠️ Execution:",
            "Intake Report",
            "Code Artifacts",
            "ISC Upholding Map",
            "Verification Artifacts",
            "State Update",
        ]),
        "Baxter" => Some(&[
            "🔬 Execution:",
            "Intake Report",
            "Algorithmic Proof",
            "Code Artifacts",
            "ISC Upholding Map",
            "Verification Artifacts",
            "State Update",
        ]),
        "Chaplin" => Some(&[
            "💽 Execution:",
            "Intake Report",
            "Data Model Analysis",
            "Code Artifacts",
            "ISC Upholding Map",
            "Verification Artifacts",
            "State Update",
        ]),
        "Metalhead" => Some(&[
            "📡 Execution:",
            "Intake Report",
            "Observability Plan",
            "Code Artifacts",
            "ISC Upholding Map",
            "Verification Artifacts",
            "State Update",
        ]),
        "Splinter" => Some(&[
            "🐀 Execution:",
            "Intake Report",
            "Documentation Brief",
            "Drafted Artefacts",
            "Cross-check Notes",
            "Verification Artifacts",
            "State Update",
        ]),
        "Tatsu" => Some(&[
            "🥷 Execution:",
            "Intake Report",
            "Threat Model",
            "Code Artifacts",
            "ISC Upholding Map",
            "Verification Artifacts",
            "State Update",
        ]),
        "Krang" => Some(&[
            "🧠 Execution:",
            "Intake Report",
            "Infrastructure Artifacts",
            "ISC Enforcement Map",
            "Verification Artifacts",
            "State Update",
        ]),
        _ => None,
    }
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

fn collect_loc(options: &StructuralCheckOptions) -> Value {
    let loc_script_path = resolve_path(&options.workspace_root, &options.loc_script_path);
    if !loc_script_path.exists() {
        return empty_object();
    }

    let command_path = loc_script_path
        .strip_prefix(&options.workspace_root)
        .map(PathBuf::from)
        .unwrap_or(loc_script_path);

    let output = match Command::new("bash")
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
