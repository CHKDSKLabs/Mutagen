use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::policy::author_stage_write_globs;
use crate::queue::{Slice, SliceQueue};

const DEFAULT_TARGET_LOC_WARNING_THRESHOLD: u32 = 300;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationLevel {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationIssue {
    pub level: ValidationLevel,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slice_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub advisory_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueValidationReport {
    pub ok: bool,
    pub error_count: usize,
    pub warning_count: usize,
    pub issues: Vec<ValidationIssue>,
}

pub fn load_queue_file(path: &Path) -> Result<SliceQueue> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read queue file at {}", display_path(path)))?;

    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse queue JSON at {}", display_path(path)))
}

pub fn validate_queue_file(path: &Path) -> Result<QueueValidationReport> {
    let queue = load_queue_file(path)?;
    Ok(validate_queue(&queue))
}

pub fn validate_queue(queue: &SliceQueue) -> QueueValidationReport {
    let mut issues = Vec::new();
    let mut seen_ids = HashSet::new();

    for slice in &queue.slices {
        if !seen_ids.insert(slice.id.clone()) {
            issues.push(issue(
                ValidationLevel::Error,
                "duplicate_slice_id",
                format!("duplicate slice id `{}`", slice.id),
                Some(slice.id.clone()),
                None,
            ));
        }
    }

    let known_slice_ids: HashSet<String> =
        queue.slices.iter().map(|slice| slice.id.clone()).collect();

    for advisory in &queue.planning_advisories {
        if advisory.id.trim().is_empty() {
            issues.push(issue(
                ValidationLevel::Error,
                "planning_advisory_missing_id",
                "planning advisory is missing `id`".to_string(),
                None,
                None,
            ));
        }

        for affected_slice in &advisory.affects_slices {
            if !known_slice_ids.contains(affected_slice) {
                issues.push(issue(
                    ValidationLevel::Error,
                    "planning_advisory_unknown_slice",
                    format!(
                        "planning advisory `{}` references unknown slice `{}`",
                        advisory.id, affected_slice
                    ),
                    Some(affected_slice.clone()),
                    Some(advisory.id.clone()),
                ));
            }
        }
    }

    for slice in &queue.slices {
        if let Err(error) = validate_slice_contract(slice) {
            issues.push(issue(
                ValidationLevel::Error,
                "slice_contract",
                error.to_string(),
                Some(slice.id.clone()),
                None,
            ));
        }

        for dependency in &slice.depends_on {
            if dependency == &slice.id {
                issues.push(issue(
                    ValidationLevel::Error,
                    "self_dependency",
                    format!("slice `{}` cannot depend on itself", slice.id),
                    Some(slice.id.clone()),
                    None,
                ));
            }

            if !known_slice_ids.contains(dependency) {
                issues.push(issue(
                    ValidationLevel::Error,
                    "unknown_dependency",
                    format!(
                        "slice `{}` depends on unknown slice `{}`",
                        slice.id, dependency
                    ),
                    Some(slice.id.clone()),
                    None,
                ));
            }
        }

        if slice.target_loc > DEFAULT_TARGET_LOC_WARNING_THRESHOLD {
            issues.push(issue(
                ValidationLevel::Warning,
                "target_loc_above_default",
                format!(
                    "slice `{}` targets {} LOC, above the default {} LOC budget",
                    slice.id, slice.target_loc, DEFAULT_TARGET_LOC_WARNING_THRESHOLD
                ),
                Some(slice.id.clone()),
                None,
            ));
        }

        if slice.human_check_needed.required && slice.human_check_needed.reason.trim().is_empty() {
            issues.push(issue(
                ValidationLevel::Error,
                "human_check_missing_reason",
                format!(
                    "slice `{}` requires a human check but gives no reason",
                    slice.id
                ),
                Some(slice.id.clone()),
                None,
            ));
        }

        if slice.human_check_needed.required && slice.human_check_needed.resolved_at.is_none() {
            issues.push(issue(
                ValidationLevel::Warning,
                "human_check_pending",
                format!(
                    "slice `{}` is gated on an unresolved human check; finalize \
                     will refuse until `update-slice --resolve-human-check` runs \
                     or `human_check_needed.required` is flipped to false",
                    slice.id
                ),
                Some(slice.id.clone()),
                None,
            ));
        }
    }

    let error_count = issues
        .iter()
        .filter(|issue| issue.level == ValidationLevel::Error)
        .count();
    let warning_count = issues
        .iter()
        .filter(|issue| issue.level == ValidationLevel::Warning)
        .count();

    QueueValidationReport {
        ok: error_count == 0,
        error_count,
        warning_count,
        issues,
    }
}

pub fn validate_slice_contract(slice: &Slice) -> Result<()> {
    if slice.id.trim().is_empty() {
        bail!("slice is missing `id`");
    }
    if slice.title.trim().is_empty() {
        bail!("slice `{}` is missing `title`", slice.id);
    }
    if slice.author_agent.trim().is_empty() {
        bail!("slice `{}` is missing `author_agent`", slice.id);
    }
    if slice.bounded_context.trim().is_empty() {
        bail!("slice `{}` is missing `bounded_context`", slice.id);
    }
    if slice.target_loc == 0 {
        bail!("slice `{}` is missing a non-zero `target_loc`", slice.id);
    }
    if slice.objective.trim().is_empty() {
        bail!("slice `{}` is missing `objective`", slice.id);
    }
    if slice.context_to_update.trim().is_empty() {
        bail!("slice `{}` is missing `context_to_update`", slice.id);
    }
    if slice.implementation_details.is_empty() {
        bail!("slice `{}` is missing `implementation_details`", slice.id);
    }
    if slice.verification_steps.acceptance.trim().is_empty() {
        bail!(
            "slice `{}` is missing `verification_steps.acceptance`",
            slice.id
        );
    }

    let has_trace = !slice.traces_to.prd.is_empty()
        || !slice.traces_to.adr.is_empty()
        || !slice.traces_to.ddd.is_empty()
        || !slice.traces_to.isc.is_empty()
        || !slice.traces_to.dsd.is_empty();

    if !has_trace {
        bail!("slice `{}` is missing `traces_to` references", slice.id);
    }

    let _ = author_stage_write_globs(slice)?;

    Ok(())
}

fn issue(
    level: ValidationLevel,
    code: &str,
    message: String,
    slice_id: Option<String>,
    advisory_id: Option<String>,
) -> ValidationIssue {
    ValidationIssue {
        level,
        code: code.to_string(),
        message,
        slice_id,
        advisory_id,
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
