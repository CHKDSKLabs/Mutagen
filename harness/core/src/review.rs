use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::notifications::{NotificationEvent, StopCondition, retry_exhausted_notification};
use crate::policy::{author_stage_write_globs, default_author_write_set, globs_cover_all};
use crate::queue::{SliceStatus, TigerClawVerdict};
use crate::review_record::parse_tiger_claw_verdict;
use crate::state::{Stage, load_active_slice};
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct ReviewDecisionOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub active_state_path: PathBuf,
    pub qa_report_path: Option<PathBuf>,
    pub latest_qa_report_path: Option<PathBuf>,
    pub slice_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ReviewDecisionResult {
    Continue {
        slice_id: String,
        tiger_claw: TigerClawVerdict,
        queue_path: String,
        qa_report_path: String,
        latest_qa_report_path: String,
        micro_correction_applied: bool,
    },
    MicroCorrection {
        slice_id: String,
        queue_path: String,
        qa_report_path: String,
        latest_qa_report_path: String,
        active_agent: String,
        suggested_fix_files: Vec<String>,
        suggested_fix_summary: String,
        attempts: u32,
        max_retries: u32,
        micro_corrections_used: u32,
        max_micro_corrections: u32,
    },
    Retry {
        slice_id: String,
        queue_path: String,
        qa_report_path: String,
        latest_qa_report_path: String,
        status: SliceStatus,
        attempts: u32,
        max_retries: u32,
        micro_corrections_used: u32,
        max_micro_corrections: u32,
        reason: String,
    },
    Escalated {
        slice_id: String,
        queue_path: String,
        qa_report_path: String,
        latest_qa_report_path: String,
        status: SliceStatus,
        attempts: u32,
        max_retries: u32,
        micro_corrections_used: u32,
        max_micro_corrections: u32,
        escalation_reason: String,
        stop_condition: StopCondition,
        notifications: Vec<NotificationEvent>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RetryContract {
    #[serde(default)]
    pub hatch_eligible: bool,
    #[serde(default)]
    pub suggested_fix_scope: SuggestedFixScope,
    #[serde(default)]
    pub suggested_fix_files: Vec<String>,
    #[serde(default)]
    pub suggested_fix_summary: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedFixScope {
    #[default]
    None,
    Mechanical,
    Reslice,
}

pub fn review_decision(options: ReviewDecisionOptions) -> Result<ReviewDecisionResult> {
    if options.slice_id.trim().is_empty() {
        bail!("missing `slice_id`");
    }

    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let queue_path = resolve_workspace_path(&workspace_root, &options.queue_path);
    let active_state_path = resolve_workspace_path(&workspace_root, &options.active_state_path);
    let qa_report_path = options
        .qa_report_path
        .as_deref()
        .map(|path| resolve_workspace_path(&workspace_root, path))
        .unwrap_or_else(|| {
            workspace_root
                .join("reviews")
                .join(&options.slice_id)
                .join("tiger-claw.md")
        });
    let latest_qa_report_path = options
        .latest_qa_report_path
        .as_deref()
        .map(|path| resolve_workspace_path(&workspace_root, path))
        .unwrap_or_else(|| workspace_root.join(".mutagen/state/tiger-claw-latest.md"));

    let mut queue = load_queue_file(&queue_path)?;
    let active_state = load_active_slice(&active_state_path)?;

    if active_state.slice_id != options.slice_id {
        bail!(
            "active slice mismatch: expected `{}`, found `{}`",
            options.slice_id,
            active_state.slice_id
        );
    }

    if active_state.stage != Stage::Review {
        bail!(
            "cannot evaluate review for slice `{}` while active stage is `{}`",
            options.slice_id,
            stage_name(active_state.stage)
        );
    }

    let qa_report = fs::read_to_string(&qa_report_path).with_context(|| {
        format!(
            "failed to read QA report at {}",
            display_path(&qa_report_path)
        )
    })?;
    let latest_qa_report = fs::read_to_string(&latest_qa_report_path).with_context(|| {
        format!(
            "failed to read latest QA report at {}",
            display_path(&latest_qa_report_path)
        )
    })?;

    if latest_qa_report.trim().is_empty() {
        bail!(
            "latest QA report at {} is empty",
            display_path(&latest_qa_report_path)
        );
    }

    // 2026-05-12 L4-Workflow-001: Bebop retried, TigerClaw re-reviewed clean,
    // but the queue still carried the prior dispatch's `defect` and the slice
    // stayed escalated until an operator hand-typed `update-slice
    // --tiger-claw clean`. Source of truth for the decision is the freshly-
    // parsed verdict; if it disagrees with the queue, the queue is the stale
    // one and we promote-and-persist before computing the action.
    let fresh_verdict_from_report = parse_tiger_claw_verdict(&qa_report);
    let mut verdict_was_promoted = false;
    if let Some(fresh) = fresh_verdict_from_report {
        let slice = queue
            .slice_mut(&options.slice_id)
            .with_context(|| format!("slice `{}` not found", options.slice_id))?;

        if slice.verdicts.tiger_claw != Some(fresh) {
            slice.verdicts.tiger_claw = Some(fresh);
            verdict_was_promoted = true;
        }
    }
    if verdict_was_promoted {
        write_json_file(&queue_path, &queue)?;
    }

    let result = {
        let slice = queue
            .slice_mut(&options.slice_id)
            .with_context(|| format!("slice `{}` not found", options.slice_id))?;

        if slice.status != SliceStatus::InProgress {
            bail!(
                "cannot evaluate review for slice `{}` from status `{}`",
                slice.id,
                slice_status_name(slice.status)
            );
        }

        let tiger_claw = slice
            .verdicts
            .tiger_claw
            .with_context(|| format!("slice `{}` is missing a Tiger Claw verdict", slice.id))?;

        match tiger_claw {
            TigerClawVerdict::Clean | TigerClawVerdict::Gap | TigerClawVerdict::Skip => {
                if active_state.micro_corrections_used > 0 {
                    slice.verdicts.micro_correction = Some(true);
                }

                ReviewDecisionResult::Continue {
                    slice_id: slice.id.clone(),
                    tiger_claw,
                    queue_path: display_path(&queue_path),
                    qa_report_path: display_path(&qa_report_path),
                    latest_qa_report_path: display_path(&latest_qa_report_path),
                    micro_correction_applied: active_state.micro_corrections_used > 0,
                }
            }
            TigerClawVerdict::Defect => {
                let retry_contract = parse_retry_contract(&qa_report).unwrap_or_default();
                let hatch_result =
                    evaluate_micro_correction_hatch(slice, &active_state, &retry_contract)?;

                if let Some(active_agent) = hatch_result.active_agent {
                    return Ok(ReviewDecisionResult::MicroCorrection {
                        slice_id: slice.id.clone(),
                        queue_path: display_path(&queue_path),
                        qa_report_path: display_path(&qa_report_path),
                        latest_qa_report_path: display_path(&latest_qa_report_path),
                        active_agent,
                        suggested_fix_files: hatch_result.suggested_fix_files,
                        suggested_fix_summary: retry_contract
                            .suggested_fix_summary
                            .trim()
                            .to_string(),
                        attempts: active_state.attempts,
                        max_retries: active_state.max_retries,
                        micro_corrections_used: active_state.micro_corrections_used,
                        max_micro_corrections: active_state.max_micro_corrections,
                    });
                }

                if active_state.attempts > active_state.max_retries {
                    let escalation_reason = format!(
                        "Tiger Claw Defect after {} attempts (micro_corrections_used: {})",
                        active_state.attempts, active_state.micro_corrections_used
                    );
                    slice.status = SliceStatus::Escalated;
                    slice.escalation_reason = Some(escalation_reason.clone());

                    ReviewDecisionResult::Escalated {
                        slice_id: slice.id.clone(),
                        queue_path: display_path(&queue_path),
                        qa_report_path: display_path(&qa_report_path),
                        latest_qa_report_path: display_path(&latest_qa_report_path),
                        status: slice.status,
                        attempts: active_state.attempts,
                        max_retries: active_state.max_retries,
                        micro_corrections_used: active_state.micro_corrections_used,
                        max_micro_corrections: active_state.max_micro_corrections,
                        escalation_reason,
                        stop_condition: StopCondition::RetryBudgetExhausted,
                        notifications: vec![retry_exhausted_notification(
                            &slice.id,
                            &slice.title,
                            active_state.attempts,
                            active_state.micro_corrections_used,
                        )],
                    }
                } else {
                    slice.status = SliceStatus::BlockedRetry;

                    ReviewDecisionResult::Retry {
                        slice_id: slice.id.clone(),
                        queue_path: display_path(&queue_path),
                        qa_report_path: display_path(&qa_report_path),
                        latest_qa_report_path: display_path(&latest_qa_report_path),
                        status: slice.status,
                        attempts: active_state.attempts,
                        max_retries: active_state.max_retries,
                        micro_corrections_used: active_state.micro_corrections_used,
                        max_micro_corrections: active_state.max_micro_corrections,
                        reason: hatch_result.reason,
                    }
                }
            }
        }
    };

    write_json_file(&queue_path, &queue)?;

    Ok(result)
}

#[derive(Debug, Default)]
struct HatchEvaluation {
    active_agent: Option<String>,
    suggested_fix_files: Vec<String>,
    reason: String,
}

fn evaluate_micro_correction_hatch(
    slice: &crate::queue::Slice,
    active_state: &crate::state::ActiveSliceState,
    retry_contract: &RetryContract,
) -> Result<HatchEvaluation> {
    if active_state.micro_corrections_used >= active_state.max_micro_corrections {
        return Ok(HatchEvaluation {
            reason: "micro_correction_budget_exhausted".to_string(),
            ..Default::default()
        });
    }

    if !retry_contract.hatch_eligible {
        return Ok(HatchEvaluation {
            reason: "retry_contract_not_hatch_eligible".to_string(),
            ..Default::default()
        });
    }

    if retry_contract.suggested_fix_scope != SuggestedFixScope::Mechanical {
        return Ok(HatchEvaluation {
            reason: "retry_contract_not_mechanical".to_string(),
            ..Default::default()
        });
    }

    let suggested_fix_files = normalize_fix_paths(&retry_contract.suggested_fix_files);
    if suggested_fix_files.is_empty() {
        return Ok(HatchEvaluation {
            reason: "retry_contract_missing_fix_files".to_string(),
            ..Default::default()
        });
    }

    let author_scope = author_stage_write_globs(slice)?;
    if globs_cover_all(&author_scope, &suggested_fix_files)? {
        return Ok(HatchEvaluation {
            active_agent: Some(slice.author_agent.clone()),
            suggested_fix_files,
            reason: "micro_correction_author_scope".to_string(),
        });
    }

    let bebop_scope = default_author_write_set("Bebop")?;
    if globs_cover_all(&bebop_scope, &suggested_fix_files)? {
        return Ok(HatchEvaluation {
            active_agent: Some("Bebop".to_string()),
            suggested_fix_files,
            reason: "micro_correction_bebop_fallback".to_string(),
        });
    }

    Ok(HatchEvaluation {
        reason: "suggested_fix_files_out_of_scope".to_string(),
        ..Default::default()
    })
}

fn parse_retry_contract(report: &str) -> Option<RetryContract> {
    let mut lines = report.lines().peekable();

    while let Some(line) = lines.next() {
        if line.trim() != "#### Retry Contract" {
            continue;
        }

        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                lines.next();
                continue;
            }
            break;
        }

        if lines.next()?.trim() != "```json" {
            return None;
        }

        let mut json_body = String::new();
        for next in lines {
            if next.trim() == "```" {
                break;
            }
            json_body.push_str(next);
            json_body.push('\n');
        }

        return serde_json::from_str(&json_body).ok();
    }

    None
}

fn normalize_fix_paths(paths: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();

    for path in paths {
        let value = path.trim().trim_matches('`').replace('\\', "/");
        if value.is_empty() || normalized.contains(&value) {
            continue;
        }
        normalized.push(value);
    }

    normalized
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(path)
            )
        })?;
    }

    let body = serde_json::to_string_pretty(value).context("failed to serialize JSON")?;
    fs::write(path, format!("{body}\n"))
        .with_context(|| format!("failed to write {}", display_path(path)))
}

fn resolve_workspace_root(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("missing `workspace_root`");
    }

    if path.exists() {
        fs::canonicalize(path)
            .with_context(|| format!("failed to resolve workspace root {}", display_path(path)))
    } else if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("failed to read current working directory")?
            .join(path))
    }
}

fn resolve_workspace_path(workspace_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Author => "author",
        Stage::StructuralCheck => "structural_check",
        Stage::Review => "review",
        Stage::StateRecord => "state_record",
    }
}

fn slice_status_name(status: SliceStatus) -> &'static str {
    match status {
        SliceStatus::Pending => "pending",
        SliceStatus::InProgress => "in_progress",
        SliceStatus::BlockedRetry => "blocked_retry",
        SliceStatus::Completed => "completed",
        SliceStatus::Escalated => "escalated",
        SliceStatus::Refused => "refused",
        SliceStatus::FinalizationFailed => "finalization_failed",
    }
}
