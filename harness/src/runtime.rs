use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::activation::{ActivateSliceOptions, activate_slice};
use crate::adapter::{HostExecutionProfile, HostKind, resolved_host_profile};
use crate::config::load_workflow_config_file;
use crate::notifications::{NotificationEvent, StopCondition, queue_clear_notification};
use crate::queue::{BlockedSlice, NextSliceSelection, SliceQueue};
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct PrepareNextOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub workflow_config_path: PathBuf,
    pub active_state_path: PathBuf,
    pub host: HostKind,
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PrepareNextResult {
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
    Stalled {
        blocked: Vec<BlockedSlice>,
        stop_condition: StopCondition,
    },
    QueueClear {
        completed_count: usize,
        stop_condition: StopCondition,
        notifications: Vec<NotificationEvent>,
    },
}

pub fn prepare_next(options: PrepareNextOptions) -> Result<PrepareNextResult> {
    let mut queue = load_queue(&options.queue_path)?;
    let workflow_config = load_workflow_config_file(&options.workflow_config_path)?;
    let host_profile = resolved_host_profile(options.host, &workflow_config);

    match queue.select_next_ready_slice() {
        NextSliceSelection::Ready { index } => {
            let activation = activate_slice(ActivateSliceOptions {
                workspace_root: &options.workspace_root,
                queue_path: &options.queue_path,
                active_state_path: &options.active_state_path,
                queue: &mut queue,
                slice_index: index,
                workflow_config,
                host: options.host,
                host_profile,
                claim_requested: true,
                dry_run: options.dry_run,
            })?;

            Ok(PrepareNextResult::Ready {
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
                queue_path: display_path(&options.queue_path),
                host: activation.host,
                degraded_capabilities: activation.degraded_capabilities,
                host_profile: activation.host_profile,
                claimed: activation.claimed,
            })
        }
        NextSliceSelection::QueueClear => {
            let completed_count = queue
                .slices
                .iter()
                .filter(|slice| slice.status == crate::queue::SliceStatus::Completed)
                .count();

            Ok(PrepareNextResult::QueueClear {
                completed_count,
                stop_condition: StopCondition::QueueClear,
                notifications: vec![queue_clear_notification(completed_count)],
            })
        }
        NextSliceSelection::Stalled { blocked } => Ok(PrepareNextResult::Stalled {
            blocked,
            stop_condition: StopCondition::QueueStalled,
        }),
    }
}

fn load_queue(path: &Path) -> Result<SliceQueue> {
    load_queue_file(path)
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
