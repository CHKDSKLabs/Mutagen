use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::policy::author_stage_write_globs;
use crate::queue::SliceStatus;
use crate::queue_readiness::require_queue_ready;
use crate::state::{Stage, load_active_slice, write_active_slice};
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct TransitionActiveSliceOptions {
    pub queue_path: PathBuf,
    pub active_state_path: PathBuf,
    pub slice_id: String,
    pub stage: Stage,
    pub active_agent: Option<String>,
    pub bump_attempts: bool,
    pub bump_micro_corrections: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransitionActiveSliceResult {
    pub queue_path: String,
    pub active_state_path: String,
    pub slice_id: String,
    pub status: SliceStatus,
    pub stage: Stage,
    pub active_agent: String,
    pub attempts: u32,
    pub micro_corrections_used: u32,
    pub allowed_write_globs: Vec<String>,
}

pub fn transition_active_slice(
    options: TransitionActiveSliceOptions,
) -> Result<TransitionActiveSliceResult> {
    if options.bump_attempts && options.bump_micro_corrections {
        bail!("cannot bump attempts and micro corrections in the same transition");
    }

    if options.stage != Stage::Author && options.active_agent.is_some() {
        bail!("active_agent override is only valid for the author stage");
    }

    let mut queue = load_queue_file(&options.queue_path)?;
    let mut active_state = load_active_slice(&options.active_state_path)?;
    verify_active_queue_readiness(&options.queue_path, &active_state)?;

    if active_state.slice_id != options.slice_id {
        bail!(
            "active slice mismatch: expected `{}`, found `{}`",
            options.slice_id,
            active_state.slice_id
        );
    }

    let result = {
        let slice = queue
            .slice_mut(&options.slice_id)
            .with_context(|| format!("slice `{}` not found", options.slice_id))?;

        if !matches!(
            slice.status,
            SliceStatus::Pending | SliceStatus::InProgress | SliceStatus::BlockedRetry
        ) {
            bail!(
                "cannot transition slice `{}` from status `{}`",
                slice.id,
                slice_status_name(slice.status)
            );
        }

        active_state.attempts = slice.attempts;
        active_state.micro_corrections_used = slice.micro_corrections_used;

        match options.stage {
            Stage::Author => {
                let allowed_write_globs = author_stage_write_globs(slice)?;
                let active_agent = options
                    .active_agent
                    .clone()
                    .unwrap_or_else(|| slice.author_agent.clone());

                if options.bump_attempts {
                    slice.attempts += 1;
                    active_state.attempts = slice.attempts;
                    if active_state.started_at_unix_ms.is_none() {
                        active_state.started_at_unix_ms = Some(now_unix_ms()?);
                    }
                }

                if options.bump_micro_corrections {
                    slice.micro_corrections_used += 1;
                    slice.verdicts.micro_corrections_used = Some(slice.micro_corrections_used);
                    active_state.micro_corrections_used = slice.micro_corrections_used;
                }

                slice.status = SliceStatus::InProgress;
                active_state.set_author_stage(active_agent, allowed_write_globs);
            }
            Stage::StructuralCheck => active_state.set_structural_check_stage(),
            Stage::Review => active_state.set_review_stage(),
            Stage::StateRecord => active_state.set_state_record_stage()?,
        }

        TransitionActiveSliceResult {
            queue_path: display_path(&options.queue_path),
            active_state_path: display_path(&options.active_state_path),
            slice_id: slice.id.clone(),
            status: slice.status,
            stage: active_state.stage,
            active_agent: active_state.active_agent.clone(),
            attempts: active_state.attempts,
            micro_corrections_used: active_state.micro_corrections_used,
            allowed_write_globs: active_state.allowed_write_globs.clone(),
        }
    };

    write_json_file(&options.queue_path, &queue)?;
    write_active_slice(&options.active_state_path, &active_state)?;

    Ok(result)
}

fn verify_active_queue_readiness(
    queue_path: &Path,
    active_state: &crate::state::ActiveSliceState,
) -> Result<()> {
    let Some(expected) = active_state.queue_readiness.as_ref() else {
        bail!(
            "active slice `{}` has no queue validation snapshot; re-prepare the slice through the harness before transitioning stages",
            active_state.slice_id
        );
    };

    let validation_path = PathBuf::from(&expected.queue_validation_path);
    let current = require_queue_ready(queue_path, &validation_path)?;

    if expected.queue_contract_hash.is_some()
        && current.queue_contract_hash != expected.queue_contract_hash
    {
        bail!(
            "queue validation changed after slice `{}` was activated; re-run /mutagen:slice before continuing",
            active_state.slice_id
        );
    }

    if expected.queue_contract_hash_basis.is_some()
        && current.queue_contract_hash_basis != expected.queue_contract_hash_basis
    {
        bail!(
            "queue validation basis changed after slice `{}` was activated; re-run /mutagen:slice before continuing",
            active_state.slice_id
        );
    }

    Ok(())
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

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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

fn now_unix_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?
        .as_millis() as u64)
}
