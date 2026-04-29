use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapter::{HostExecutionProfile, HostKind};
use crate::config::WorkflowConfig;
use crate::evidence::{render_evidence_bundle, write_evidence_bundle};
use crate::queue::{Slice, SliceQueue};
use crate::state::{ActiveSliceState, write_active_slice};
use crate::validation::validate_slice_contract;

#[derive(Debug, Clone)]
pub struct PreparedSliceActivation {
    pub slice: Slice,
    pub active_state_path: String,
    pub evidence_bundle_path: String,
    pub host: HostKind,
    pub degraded_capabilities: Vec<String>,
    pub host_profile: HostExecutionProfile,
    pub claimed: bool,
}

#[derive(Debug)]
pub struct ActivateSliceOptions<'a> {
    pub workspace_root: &'a Path,
    pub queue_path: &'a Path,
    pub active_state_path: &'a Path,
    pub queue: &'a mut SliceQueue,
    pub slice_index: usize,
    pub workflow_config: WorkflowConfig,
    pub host: HostKind,
    pub host_profile: HostExecutionProfile,
    pub claim_requested: bool,
    pub dry_run: bool,
}

pub fn activate_slice(options: ActivateSliceOptions<'_>) -> Result<PreparedSliceActivation> {
    let slice = options
        .queue
        .slices
        .get(options.slice_index)
        .cloned()
        .context("slice index was out of bounds")?;

    validate_slice_contract(&slice)?;

    let degraded_capabilities = options.host_profile.degraded_features.clone();
    let evidence_bundle_path = evidence_bundle_path_for(options.active_state_path, &slice.id);
    let evidence_bundle_path_display =
        display_path_relative_to_workspace(options.workspace_root, &evidence_bundle_path);
    let evidence_bundle = render_evidence_bundle(options.workspace_root, &slice)?;
    let active_state = ActiveSliceState::from_slice(
        &slice,
        options.workflow_config,
        options.host,
        degraded_capabilities.clone(),
        evidence_bundle_path_display.clone(),
    )?;

    if !options.dry_run {
        if options.claim_requested {
            options.queue.claim_slice(options.slice_index);
            write_json_file(options.queue_path, options.queue)?;
        }

        write_evidence_bundle(&evidence_bundle_path, &evidence_bundle)?;
        write_active_slice(options.active_state_path, &active_state)?;
    }

    Ok(PreparedSliceActivation {
        slice,
        active_state_path: display_path(options.active_state_path),
        evidence_bundle_path: evidence_bundle_path_display,
        host: options.host_profile.host,
        degraded_capabilities,
        host_profile: options.host_profile,
        claimed: !options.dry_run && options.claim_requested,
    })
}

pub fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn display_path_relative_to_workspace(workspace_root: &Path, path: &Path) -> String {
    let normalized_root = normalize_path_separators(workspace_root);
    let normalized_path = normalize_path_separators(path);
    let normalized_root = normalized_root.trim_end_matches('/');

    normalized_path
        .strip_prefix(&format!("{normalized_root}/"))
        .map(str::to_string)
        .unwrap_or(normalized_path)
}

pub fn evidence_bundle_path_for(active_state_path: &Path, slice_id: &str) -> PathBuf {
    let evidence_dir = active_state_path
        .parent()
        .map(|parent| parent.join("evidence"))
        .unwrap_or_else(|| PathBuf::from(".mutagen/state/evidence"));

    evidence_dir.join(format!("{}.md", safe_file_name(slice_id)))
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

fn normalize_path_separators(path: &Path) -> String {
    let normalized = display_path(path).replace('\\', "/");
    normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .to_string()
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
