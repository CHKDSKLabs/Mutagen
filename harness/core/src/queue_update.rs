use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::Path;

use crate::queue::{
    BishopVerdict, KaraiStructuralVerdict, SliceStatus, SliceVerdicts, TigerClawVerdict,
};
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct UpdateSliceOptions {
    pub queue_path: std::path::PathBuf,
    pub slice_id: String,
    pub status: Option<SliceStatus>,
    pub attempts: Option<u32>,
    pub micro_corrections_used: Option<u32>,
    pub karai_structural: Option<KaraiStructuralVerdict>,
    pub bishop: Option<BishopVerdict>,
    pub tiger_claw: Option<TigerClawVerdict>,
    pub micro_correction: Option<bool>,
    pub completed_at: Option<String>,
    pub clear_completed_at: bool,
    pub escalation_reason: Option<String>,
    pub clear_escalation_reason: bool,
    pub human_check_resolved_at: Option<String>,
    pub clear_human_check_resolved_at: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateSliceResult {
    pub queue_path: String,
    pub slice_id: String,
    pub status: SliceStatus,
    pub attempts: u32,
    pub micro_corrections_used: u32,
    #[serde(skip_serializing_if = "SliceVerdicts::is_empty")]
    pub verdicts: SliceVerdicts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalation_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub human_check_resolved_at: Option<String>,
    pub human_check_required: bool,
}

pub fn update_slice(options: UpdateSliceOptions) -> Result<UpdateSliceResult> {
    if options.clear_completed_at && options.completed_at.is_some() {
        bail!("cannot set and clear `completed_at` in the same update");
    }

    if options.clear_escalation_reason && options.escalation_reason.is_some() {
        bail!("cannot set and clear `escalation_reason` in the same update");
    }

    if options.clear_human_check_resolved_at && options.human_check_resolved_at.is_some() {
        bail!("cannot set and clear `human_check_needed.resolved_at` in the same update");
    }

    let mut queue = load_queue_file(&options.queue_path)?;
    let result = {
        let slice = queue
            .slice_mut(&options.slice_id)
            .with_context(|| format!("slice `{}` not found", options.slice_id))?;

        if let Some(status) = options.status {
            slice.status = status;
        }

        if let Some(attempts) = options.attempts {
            slice.attempts = attempts;
        }

        if let Some(micro_corrections_used) = options.micro_corrections_used {
            slice.micro_corrections_used = micro_corrections_used;
            slice.verdicts.micro_corrections_used = Some(micro_corrections_used);
        }

        if let Some(verdict) = options.karai_structural {
            slice.verdicts.karai_structural = Some(verdict);
        }

        if let Some(verdict) = options.bishop {
            slice.verdicts.bishop = Some(verdict);
        }

        if let Some(verdict) = options.tiger_claw {
            slice.verdicts.tiger_claw = Some(verdict);
        }

        if let Some(micro_correction) = options.micro_correction {
            slice.verdicts.micro_correction = Some(micro_correction);
        }

        if options.clear_completed_at {
            slice.completed_at = None;
        } else if let Some(completed_at) = options.completed_at {
            slice.completed_at = Some(completed_at);
        }

        if options.clear_escalation_reason {
            slice.escalation_reason = None;
        } else if let Some(escalation_reason) = options.escalation_reason {
            slice.escalation_reason = Some(escalation_reason);
        }

        if options.clear_human_check_resolved_at {
            slice.human_check_needed.resolved_at = None;
        } else if let Some(resolved_at) = options.human_check_resolved_at {
            slice.human_check_needed.resolved_at = Some(resolved_at);
        }

        UpdateSliceResult {
            queue_path: display_path(&options.queue_path),
            slice_id: slice.id.clone(),
            status: slice.status,
            attempts: slice.attempts,
            micro_corrections_used: slice.micro_corrections_used,
            verdicts: slice.verdicts.clone(),
            completed_at: slice.completed_at.clone(),
            escalation_reason: slice.escalation_reason.clone(),
            human_check_resolved_at: slice.human_check_needed.resolved_at.clone(),
            human_check_required: slice.human_check_needed.required,
        }
    };

    write_json_file(&options.queue_path, &queue)?;

    Ok(result)
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
