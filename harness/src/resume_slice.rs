use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::activation::{ActivateSliceOptions, PreparedSliceReady, activate_slice};
use crate::adapter::{HostKind, resolved_host_profile};
use crate::config::load_workflow_config_file;
use crate::policy::author_stage_write_globs;
use crate::queue::{Slice, SliceStatus};
use crate::state::{Stage, load_active_slice, write_active_slice};
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct ResumeSliceOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub workflow_config_path: PathBuf,
    pub active_state_path: PathBuf,
    pub slice_id: String,
    pub from_stage: Stage,
    pub host: HostKind,
}

#[derive(Debug, Serialize)]
pub struct ResumeSliceResult {
    pub slice_id: String,
    pub from_stage: Stage,
    pub previous_active_slice_id: Option<String>,
    pub previous_stage: Option<Stage>,
    pub status: SliceStatus,
    pub active_agent: String,
    pub allowed_write_globs: Vec<String>,
    pub prepared: Box<PreparedSliceReady>,
}

pub fn resume_slice(options: ResumeSliceOptions) -> Result<ResumeSliceResult> {
    if options.slice_id.trim().is_empty() {
        bail!("missing `slice_id`");
    }

    // Surface the prior active-slice for the human's situational awareness -- if they
    // were unsticking a different slice from the one in active-slice.json, the JSON
    // result tells them exactly what got clobbered.
    let previous = load_active_slice(&options.active_state_path).ok();
    let previous_active_slice_id = previous.as_ref().map(|state| state.slice_id.clone());
    let previous_stage = previous.as_ref().map(|state| state.stage);

    let mut queue = load_queue_file(&options.queue_path)?;
    let workflow_config = load_workflow_config_file(&options.workflow_config_path)?;
    let host_profile = resolved_host_profile(options.host, &workflow_config);

    let slice_index = queue
        .slices
        .iter()
        .position(|slice| slice.id == options.slice_id)
        .with_context(|| {
            format!(
                "slice `{}` not found in {}",
                options.slice_id,
                display_path(&options.queue_path)
            )
        })?;

    {
        let slice = queue.slices.get(slice_index).expect("index just resolved");
        match slice.status {
            SliceStatus::Pending | SliceStatus::InProgress | SliceStatus::BlockedRetry => {}
            terminal => bail!(
                "cannot resume slice `{}` with terminal status `{}` -- update the queue first",
                slice.id,
                slice_status_name(terminal)
            ),
        }
    }

    let activation = activate_slice(ActivateSliceOptions {
        workspace_root: &options.workspace_root,
        queue_path: &options.queue_path,
        active_state_path: &options.active_state_path,
        queue: &mut queue,
        slice_index,
        workflow_config: workflow_config.clone(),
        host: options.host,
        host_profile,
        // claim_requested == bump status into InProgress in the queue; we always want
        // that on resume so a stuck Pending row doesn't stay Pending after we hand it
        // to the executor again.
        claim_requested: true,
        dry_run: false,
    })?;

    // activate_slice wrote a fresh ActiveSliceState at stage=Author. Re-load it,
    // pivot to the requested stage, and persist. Two writes is wasteful but the
    // alternative is plumbing a stage override through activate_slice, which spreads
    // resume-specific knowledge into the hot path used by every cohort dispatch.
    let mut active_state = load_active_slice(&options.active_state_path)?;
    apply_from_stage(&mut active_state, options.from_stage, &activation.slice)?;
    write_active_slice(&options.active_state_path, &active_state)?;

    let queue_path_display = display_path(&options.queue_path);
    // activate_slice cloned the slice before bumping it via claim_slice, so its
    // copy still says "pending". Re-read the queue we just persisted to surface
    // the actual landed status.
    let slice_status = queue
        .slices
        .get(slice_index)
        .map(|slice| slice.status)
        .unwrap_or(activation.slice.status);
    let prepared = Box::new(PreparedSliceReady::from_activation(
        activation,
        queue_path_display,
    ));

    Ok(ResumeSliceResult {
        slice_id: prepared.slice_id.clone(),
        from_stage: options.from_stage,
        previous_active_slice_id,
        previous_stage,
        status: slice_status,
        active_agent: active_state.active_agent.clone(),
        allowed_write_globs: active_state.allowed_write_globs.clone(),
        prepared,
    })
}

fn apply_from_stage(
    active_state: &mut crate::state::ActiveSliceState,
    stage: Stage,
    slice: &Slice,
) -> Result<()> {
    match stage {
        Stage::Author => {
            let globs = author_stage_write_globs(slice)?;
            active_state.set_author_stage(slice.author_agent.clone(), globs);
        }
        Stage::StructuralCheck => active_state.set_structural_check_stage(),
        Stage::Review => active_state.set_review_stage(),
        Stage::StateRecord => active_state.set_state_record_stage(),
    }
    Ok(())
}

fn slice_status_name(status: SliceStatus) -> &'static str {
    match status {
        SliceStatus::Pending => "pending",
        SliceStatus::InProgress => "in_progress",
        SliceStatus::BlockedRetry => "blocked_retry",
        SliceStatus::Completed => "completed",
        SliceStatus::Escalated => "escalated",
        SliceStatus::Refused => "refused",
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
