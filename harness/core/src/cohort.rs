use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::adapter::{HostExecutionProfile, HostKind, ParallelDispatchMode, resolved_host_profile};
use crate::config::load_workflow_config_file;
use crate::evidence::{render_evidence_bundle, write_evidence_bundle};
use crate::notifications::{NotificationEvent, StopCondition, queue_clear_notification};
use crate::policy::{dedupe_globs, default_author_write_set};
use crate::queue::{BlockedSlice, NextSliceSelection, Slice, SliceQueue, SliceStatus};
use crate::queue_readiness::require_queue_ready;
use crate::validation::{load_queue_file, validate_slice_contract};

#[derive(Debug, Clone)]
pub struct PrepareCohortOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub queue_validation_path: PathBuf,
    pub workflow_config_path: PathBuf,
    pub host: HostKind,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CohortSlice {
    pub slice_id: String,
    pub title: String,
    pub author_agent: String,
    pub layer: u32,
    pub bounded_context: String,
    pub objective: String,
    pub review_required: bool,
    pub attempts: u32,
    pub context_to_update: String,
    pub write_set: Vec<String>,
    pub adjacent_scope_allowed: Vec<String>,
    pub depends_on: Vec<String>,
    pub selection_scope: Vec<String>,
    pub evidence_bundle_path: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeferredReason {
    LayerMismatch,
    WriteSetConflict,
    CohortLimitReached,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeferredSlice {
    pub slice_id: String,
    pub layer: u32,
    pub reason: DeferredReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflicting_slice_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflicting_glob: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PrepareCohortResult {
    SerialOnly {
        host: HostKind,
        host_profile: HostExecutionProfile,
        message: String,
    },
    Ready {
        cohort_layer: u32,
        requested_max_parallel_slices: u32,
        effective_max_parallel_slices: u32,
        queue_path: String,
        host: HostKind,
        host_profile: HostExecutionProfile,
        prepared: bool,
        cohort: Vec<CohortSlice>,
        deferred: Vec<DeferredSlice>,
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

pub fn prepare_cohort(options: PrepareCohortOptions) -> Result<PrepareCohortResult> {
    let _queue_readiness =
        require_queue_ready(&options.queue_path, &options.queue_validation_path)?;
    let queue = load_queue_file(&options.queue_path)?;
    let workflow_config = load_workflow_config_file(&options.workflow_config_path)?;
    let host_profile = resolved_host_profile(options.host, &workflow_config);

    if host_profile.parallel_dispatch != ParallelDispatchMode::BoundedCohort {
        return Ok(PrepareCohortResult::SerialOnly {
            host: host_profile.host,
            host_profile,
            message: "host execution profile resolved to serial_only; bounded cohort dispatch is unavailable".to_string(),
        });
    }

    match queue.select_next_ready_slice() {
        NextSliceSelection::Ready { index } => {
            let anchor = queue
                .slices
                .get(index)
                .cloned()
                .context("ready slice index was out of bounds")?;

            validate_slice_contract(&anchor)?;

            let anchor_scope = cohort_selection_scope(&anchor)?;
            let anchor_evidence_path =
                evidence_bundle_path_for(&options.workspace_root, &anchor.id);
            let anchor_evidence_display =
                display_path_relative_to_workspace(&options.workspace_root, &anchor_evidence_path);

            let mut selected_slices = vec![SelectedSlice {
                slice: anchor.clone(),
                selection_scope: anchor_scope,
                evidence_bundle_path: anchor_evidence_path,
                evidence_bundle_display: anchor_evidence_display,
            }];
            let mut deferred = Vec::new();

            for candidate in queue.slices.iter().skip(index + 1) {
                if !candidate.status.is_ready_candidate() {
                    continue;
                }

                let unmet_dependencies = unmet_dependencies(&queue, candidate);
                if !unmet_dependencies.is_empty() {
                    continue;
                }

                if candidate.has_pending_human_check() {
                    continue;
                }

                validate_slice_contract(candidate)?;

                if candidate.layer != anchor.layer {
                    deferred.push(DeferredSlice {
                        slice_id: candidate.id.clone(),
                        layer: candidate.layer,
                        reason: DeferredReason::LayerMismatch,
                        conflicting_slice_id: None,
                        conflicting_glob: None,
                    });
                    continue;
                }

                if selected_slices.len() >= host_profile.effective_max_parallel_slices as usize {
                    deferred.push(DeferredSlice {
                        slice_id: candidate.id.clone(),
                        layer: candidate.layer,
                        reason: DeferredReason::CohortLimitReached,
                        conflicting_slice_id: None,
                        conflicting_glob: None,
                    });
                    continue;
                }

                let candidate_scope = cohort_selection_scope(candidate)?;
                if let Some((conflicting_slice_id, conflicting_glob)) =
                    first_scope_conflict(&selected_slices, &candidate_scope)
                {
                    deferred.push(DeferredSlice {
                        slice_id: candidate.id.clone(),
                        layer: candidate.layer,
                        reason: DeferredReason::WriteSetConflict,
                        conflicting_slice_id: Some(conflicting_slice_id),
                        conflicting_glob: Some(conflicting_glob),
                    });
                    continue;
                }

                let evidence_bundle_path =
                    evidence_bundle_path_for(&options.workspace_root, &candidate.id);
                let evidence_bundle_display = display_path_relative_to_workspace(
                    &options.workspace_root,
                    &evidence_bundle_path,
                );

                selected_slices.push(SelectedSlice {
                    slice: candidate.clone(),
                    selection_scope: candidate_scope,
                    evidence_bundle_path,
                    evidence_bundle_display,
                });
            }

            if !options.dry_run {
                for selected in &selected_slices {
                    let evidence_bundle =
                        render_evidence_bundle(&options.workspace_root, &selected.slice)?;
                    write_evidence_bundle(&selected.evidence_bundle_path, &evidence_bundle)?;
                }
            }

            Ok(PrepareCohortResult::Ready {
                cohort_layer: anchor.layer,
                requested_max_parallel_slices: host_profile.requested_max_parallel_slices,
                effective_max_parallel_slices: host_profile.effective_max_parallel_slices,
                queue_path: display_path(&options.queue_path),
                host: host_profile.host,
                host_profile,
                prepared: !options.dry_run,
                cohort: selected_slices
                    .into_iter()
                    .map(|selected| CohortSlice {
                        slice_id: selected.slice.id,
                        title: selected.slice.title,
                        author_agent: selected.slice.author_agent,
                        layer: selected.slice.layer,
                        bounded_context: selected.slice.bounded_context,
                        objective: selected.slice.objective,
                        review_required: selected.slice.review_required,
                        attempts: selected.slice.attempts,
                        context_to_update: selected.slice.context_to_update,
                        write_set: selected.slice.write_set,
                        adjacent_scope_allowed: selected.slice.adjacent_scope_allowed,
                        depends_on: selected.slice.depends_on,
                        selection_scope: selected.selection_scope,
                        evidence_bundle_path: selected.evidence_bundle_display,
                    })
                    .collect(),
                deferred,
            })
        }
        NextSliceSelection::QueueClear => {
            let completed_count = queue
                .slices
                .iter()
                .filter(|slice| slice.status == SliceStatus::Completed)
                .count();

            Ok(PrepareCohortResult::QueueClear {
                completed_count,
                stop_condition: StopCondition::QueueClear,
                notifications: vec![queue_clear_notification(completed_count)],
            })
        }
        NextSliceSelection::Stalled { blocked } => Ok(PrepareCohortResult::Stalled {
            blocked,
            stop_condition: StopCondition::QueueStalled,
        }),
    }
}

#[derive(Debug, Clone)]
struct SelectedSlice {
    slice: Slice,
    selection_scope: Vec<String>,
    evidence_bundle_path: PathBuf,
    evidence_bundle_display: String,
}

fn cohort_selection_scope(slice: &Slice) -> Result<Vec<String>> {
    let mut globs = if slice.write_set.is_empty() {
        default_author_write_set(&slice.author_agent)?
    } else {
        slice.write_set.clone()
    };

    if slice.status == SliceStatus::BlockedRetry || slice.attempts > 0 {
        globs.extend(slice.adjacent_scope_allowed.clone());
    }

    Ok(dedupe_globs(globs))
}

fn unmet_dependencies(queue: &SliceQueue, candidate: &Slice) -> Vec<String> {
    candidate
        .depends_on
        .iter()
        .filter(|dependency| {
            queue
                .slices
                .iter()
                .find(|slice| slice.id == dependency.as_str())
                .map(|slice| slice.status != SliceStatus::Completed)
                .unwrap_or(true)
        })
        .cloned()
        .collect()
}

fn first_scope_conflict(
    selected_slices: &[SelectedSlice],
    candidate_scope: &[String],
) -> Option<(String, String)> {
    for selected in selected_slices {
        if let Some(conflicting_glob) =
            first_glob_overlap(&selected.selection_scope, candidate_scope)
        {
            return Some((selected.slice.id.clone(), conflicting_glob));
        }
    }

    None
}

fn first_glob_overlap(left: &[String], right: &[String]) -> Option<String> {
    for left_glob in left {
        for right_glob in right {
            if glob_pair_overlaps(left_glob, right_glob) {
                return Some(format!("{left_glob} <-> {right_glob}"));
            }
        }
    }

    None
}

fn glob_pair_overlaps(left: &str, right: &str) -> bool {
    let left = normalize_glob(left);
    let right = normalize_glob(right);

    if left == right {
        return true;
    }

    let left_wild = has_glob_meta(&left);
    let right_wild = has_glob_meta(&right);

    if !left_wild && !right_wild {
        return false;
    }

    if !left_wild {
        return glob_matches_literal(&right, &left);
    }

    if !right_wild {
        return glob_matches_literal(&left, &right);
    }

    let left_prefix = literal_prefix(&left);
    let right_prefix = literal_prefix(&right);

    if left_prefix.is_empty() || right_prefix.is_empty() {
        return true;
    }

    left_prefix.starts_with(&right_prefix) || right_prefix.starts_with(&left_prefix)
}

fn glob_matches_literal(glob: &str, literal: &str) -> bool {
    globset::GlobBuilder::new(glob)
        .literal_separator(true)
        .build()
        .map(|compiled| compiled.compile_matcher().is_match(literal))
        .unwrap_or(true)
}

fn has_glob_meta(value: &str) -> bool {
    value.contains('*') || value.contains('?') || value.contains('[') || value.contains('{')
}

fn literal_prefix(value: &str) -> String {
    value
        .chars()
        .take_while(|ch| !matches!(ch, '*' | '?' | '[' | '{'))
        .collect()
}

fn normalize_glob(value: &str) -> String {
    value.trim().replace('\\', "/")
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

fn evidence_bundle_path_for(workspace_root: &Path, slice_id: &str) -> PathBuf {
    workspace_root
        .join(".mutagen/state/evidence")
        .join(format!("{}.md", safe_file_name(slice_id)))
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
