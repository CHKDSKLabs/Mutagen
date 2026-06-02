use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::activation::{ActivateSliceOptions, PreparedSliceReady, activate_slice};
use crate::adapter::{HostKind, resolved_host_profile};
use crate::config::load_workflow_config_file;
use crate::notifications::{NotificationEvent, StopCondition, queue_clear_notification};
use crate::queue::{BlockedSlice, NextSliceSelection, SliceQueue};
use crate::queue_readiness::require_queue_ready;
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct PrepareNextOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub queue_validation_path: PathBuf,
    pub workflow_config_path: PathBuf,
    pub active_state_path: PathBuf,
    pub host: HostKind,
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PrepareNextResult {
    Ready {
        #[serde(flatten)]
        prepared: Box<PreparedSliceReady>,
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
    let queue_readiness = require_queue_ready(&options.queue_path, &options.queue_validation_path)?;
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
                queue_readiness,
                claim_requested: true,
                dry_run: options.dry_run,
            })?;

            let queue_path = display_path(&options.queue_path);
            Ok(PrepareNextResult::Ready {
                prepared: Box::new(PreparedSliceReady::from_activation(activation, queue_path)),
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
