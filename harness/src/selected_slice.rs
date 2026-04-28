use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::activation::{ActivateSliceOptions, PreparedSliceActivation, activate_slice};
use crate::adapter::{HostExecutionProfile, HostKind, resolved_host_profile};
use crate::config::load_workflow_config_file;
use crate::queue::{SliceQueue, SliceStatus};
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct PrepareSelectedSliceOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub workflow_config_path: PathBuf,
    pub active_state_path: PathBuf,
    pub slice_id: String,
    pub host: HostKind,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelectedSliceBlockReason {
    NotReady,
    UnmetDependencies,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PrepareSelectedSliceResult {
    Ready {
        slice_id: String,
        title: String,
        author_agent: String,
        layer: u32,
        bounded_context: String,
        objective: String,
        review_required: bool,
        attempts: u32,
        context_to_update: String,
        write_set: Vec<String>,
        adjacent_scope_allowed: Vec<String>,
        depends_on: Vec<String>,
        active_state_path: String,
        evidence_bundle_path: String,
        queue_path: String,
        host: HostKind,
        degraded_capabilities: Vec<String>,
        host_profile: HostExecutionProfile,
        claimed: bool,
    },
    Blocked {
        slice_id: String,
        reason: SelectedSliceBlockReason,
        current_status: SliceStatus,
        unmet_dependencies: Vec<String>,
    },
}

pub fn prepare_selected_slice(
    options: PrepareSelectedSliceOptions,
) -> Result<PrepareSelectedSliceResult> {
    let mut queue = load_queue(&options.queue_path)?;
    let workflow_config = load_workflow_config_file(&options.workflow_config_path)?;
    let host_profile = resolved_host_profile(options.host, &workflow_config);

    let slice_index = queue
        .slices
        .iter()
        .position(|slice| slice.id == options.slice_id)
        .with_context(|| {
            format!(
                "slice `{}` was not found in {}",
                options.slice_id,
                options.queue_path.to_string_lossy()
            )
        })?;

    let slice = queue
        .slices
        .get(slice_index)
        .cloned()
        .context("selected slice index was out of bounds")?;

    let unmet_dependencies = queue.unmet_dependencies_for(&slice);
    if !unmet_dependencies.is_empty() {
        return Ok(PrepareSelectedSliceResult::Blocked {
            slice_id: slice.id,
            reason: SelectedSliceBlockReason::UnmetDependencies,
            current_status: slice.status,
            unmet_dependencies,
        });
    }

    if !matches!(
        slice.status,
        SliceStatus::Pending | SliceStatus::BlockedRetry | SliceStatus::InProgress
    ) {
        return Ok(PrepareSelectedSliceResult::Blocked {
            slice_id: slice.id,
            reason: SelectedSliceBlockReason::NotReady,
            current_status: slice.status,
            unmet_dependencies: Vec::new(),
        });
    }

    let activation = activate_slice(ActivateSliceOptions {
        workspace_root: &options.workspace_root,
        queue_path: &options.queue_path,
        active_state_path: &options.active_state_path,
        queue: &mut queue,
        slice_index,
        workflow_config,
        host: options.host,
        host_profile,
        claim_requested: slice.status != SliceStatus::InProgress,
        dry_run: options.dry_run,
    })?;

    Ok(ready_result(&options.queue_path, activation))
}

fn ready_result(
    queue_path: &Path,
    activation: PreparedSliceActivation,
) -> PrepareSelectedSliceResult {
    PrepareSelectedSliceResult::Ready {
        slice_id: activation.slice.id,
        title: activation.slice.title,
        author_agent: activation.slice.author_agent,
        layer: activation.slice.layer,
        bounded_context: activation.slice.bounded_context,
        objective: activation.slice.objective,
        review_required: activation.slice.review_required,
        attempts: activation.slice.attempts,
        context_to_update: activation.slice.context_to_update,
        write_set: activation.slice.write_set,
        adjacent_scope_allowed: activation.slice.adjacent_scope_allowed,
        depends_on: activation.slice.depends_on,
        active_state_path: activation.active_state_path,
        evidence_bundle_path: activation.evidence_bundle_path,
        queue_path: queue_path.to_string_lossy().into_owned(),
        host: activation.host,
        degraded_capabilities: activation.degraded_capabilities,
        host_profile: activation.host_profile,
        claimed: activation.claimed,
    }
}

fn load_queue(path: &Path) -> Result<SliceQueue> {
    load_queue_file(path)
}
