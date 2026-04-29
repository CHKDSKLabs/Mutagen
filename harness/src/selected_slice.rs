use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::activation::{
    ActivateSliceOptions, PreparedSliceActivation, PreparedSliceReady, activate_slice,
};
use crate::adapter::{HostKind, resolved_host_profile};
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
        #[serde(flatten)]
        prepared: Box<PreparedSliceReady>,
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
    let queue_path = queue_path.to_string_lossy().into_owned();
    PrepareSelectedSliceResult::Ready {
        prepared: Box::new(PreparedSliceReady::from_activation(activation, queue_path)),
    }
}

fn load_queue(path: &Path) -> Result<SliceQueue> {
    load_queue_file(path)
}
