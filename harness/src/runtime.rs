use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapter::{HostExecutionProfile, HostKind, resolved_host_profile};
use crate::config::load_workflow_config_file;
use crate::evidence::{render_evidence_bundle, write_evidence_bundle};
use crate::notifications::{NotificationEvent, StopCondition, queue_clear_notification};
use crate::queue::{BlockedSlice, NextSliceSelection, SliceQueue};
use crate::state::ActiveSliceState;
use crate::validation::{load_queue_file, validate_slice_contract};

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
            let slice = queue
                .slices
                .get(index)
                .cloned()
                .context("ready slice index was out of bounds")?;

            validate_slice_contract(&slice)?;

            let degraded_capabilities = host_profile.degraded_features.clone();
            let evidence_bundle_path =
                evidence_bundle_path_for(&options.active_state_path, &slice.id);
            let evidence_bundle_path_display =
                display_path_relative_to_workspace(&options.workspace_root, &evidence_bundle_path);
            let evidence_bundle = render_evidence_bundle(&options.workspace_root, &slice)?;
            let active_state = ActiveSliceState::from_slice(
                &slice,
                workflow_config,
                options.host,
                degraded_capabilities.clone(),
                evidence_bundle_path_display.clone(),
            )?;

            if !options.dry_run {
                queue.claim_slice(index);
                write_json_file(&options.queue_path, &queue)?;
                write_evidence_bundle(&evidence_bundle_path, &evidence_bundle)?;
                write_json_file(&options.active_state_path, &active_state)?;
            }

            Ok(PrepareNextResult::Ready {
                slice_id: slice.id,
                title: slice.title,
                author_agent: slice.author_agent,
                layer: slice.layer,
                bounded_context: slice.bounded_context,
                objective: slice.objective,
                review_required: slice.review_required,
                attempts: slice.attempts,
                context_to_update: slice.context_to_update,
                write_set: slice.write_set,
                adjacent_scope_allowed: slice.adjacent_scope_allowed,
                depends_on: slice.depends_on,
                active_state_path: display_path(&options.active_state_path),
                evidence_bundle_path: evidence_bundle_path_display,
                queue_path: display_path(&options.queue_path),
                host: host_profile.host,
                degraded_capabilities,
                host_profile,
                claimed: !options.dry_run,
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

fn display_path_relative_to_workspace(workspace_root: &Path, path: &Path) -> String {
    let normalized_root = normalize_path_separators(workspace_root);
    let normalized_path = normalize_path_separators(path);
    let normalized_root = normalized_root.trim_end_matches('/');

    normalized_path
        .strip_prefix(&format!("{normalized_root}/"))
        .map(str::to_string)
        .unwrap_or(normalized_path)
}

fn normalize_path_separators(path: &Path) -> String {
    let normalized = display_path(path).replace('\\', "/");
    normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .to_string()
}

fn evidence_bundle_path_for(active_state_path: &Path, slice_id: &str) -> PathBuf {
    let evidence_dir = active_state_path
        .parent()
        .map(|parent| parent.join("evidence"))
        .unwrap_or_else(|| PathBuf::from(".mutagen/state/evidence"));

    evidence_dir.join(format!("{}.md", safe_file_name(slice_id)))
}

fn safe_file_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
