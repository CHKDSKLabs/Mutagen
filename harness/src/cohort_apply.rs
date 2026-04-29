use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::cohort_dispatch::DispatchedCohortMember;
use crate::cohort_reconcile::{
    CompletedEntry, QueueSyncPatch, ReconcileCohortMemberOptions, ReconcileCohortMemberResult,
    reconcile_cohort_member,
};
use crate::cohort_result::CollectCohortMemberResult;
use crate::queue_update::{UpdateSliceOptions, update_slice};
use crate::state_update::{ApplyStateUpdateOptions, apply_state_update_for_slice};

#[derive(Debug, Clone)]
pub struct ApplyCohortDispatchOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub dispatch_log_path: PathBuf,
    pub member_json: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ApplyCohortDispatchResult {
    Completed {
        completed_count: usize,
        completed_slices: Vec<CompletedEntry>,
        completion_markers: Vec<String>,
    },
    Escalated {
        slice_id: String,
        worktree_path: String,
        completed_count: usize,
        completed_slices: Vec<CompletedEntry>,
        completion_markers: Vec<String>,
        terminal: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        stage: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stop_condition: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        conflicting_slice_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        conflicting_path: Option<String>,
    },
    Failed {
        slice_id: String,
        worktree_path: String,
        completed_count: usize,
        completed_slices: Vec<CompletedEntry>,
        completion_markers: Vec<String>,
        message: String,
    },
}

pub fn apply_cohort_dispatch(
    options: ApplyCohortDispatchOptions,
) -> Result<ApplyCohortDispatchResult> {
    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let queue_path = resolve_workspace_path(&workspace_root, &options.queue_path);
    let dispatch_log_path = resolve_workspace_path(&workspace_root, &options.dispatch_log_path);

    if options.member_json.is_empty() {
        bail!("at least one `member_json` entry is required");
    }

    let members = parse_dispatched_members(&options.member_json)?;
    let mut completed_slices = Vec::new();
    let mut merged_path_owners = HashMap::new();

    for member in members {
        match &member.outcome {
            CollectCohortMemberResult::Failed { message, .. } => {
                return Ok(ApplyCohortDispatchResult::Failed {
                    slice_id: member.slice_id,
                    worktree_path: member.worktree_path,
                    completed_count: completed_slices.len(),
                    completion_markers: completion_markers(&completed_slices),
                    completed_slices,
                    message: message.clone(),
                });
            }
            CollectCohortMemberResult::Ready { run_output, .. } => {
                let reconcile_output = reconcile_cohort_member(ReconcileCohortMemberOptions {
                    workspace_root: workspace_root.clone(),
                    worktree_root: PathBuf::from(&member.worktree_path),
                    slice_id: member.slice_id.clone(),
                    run_output_path: PathBuf::from(&member.result_path),
                    merged_path_owners: merged_path_owner_args(&merged_path_owners),
                })?;

                match reconcile_output {
                    ReconcileCohortMemberResult::Completed {
                        slice_id,
                        worktree_path,
                        imports,
                        queue_sync,
                        completed_entry,
                        author_output_path,
                    } => {
                        apply_imports(&workspace_root, Path::new(&worktree_path), &imports)?;
                        apply_state_update_for_slice(ApplyStateUpdateOptions {
                            workspace_root: workspace_root.clone(),
                            queue_path: queue_path.clone(),
                            slice_id: slice_id.clone(),
                            author_output_path: Some(PathBuf::from(&author_output_path)),
                        })?;
                        sync_queue_patch(&queue_path, &slice_id, &queue_sync)?;
                        append_dispatch_log_entry(
                            &workspace_root,
                            &dispatch_log_path,
                            Path::new(&worktree_path),
                            &slice_id,
                        )?;
                        record_merged_paths(&mut merged_path_owners, &slice_id, &imports);
                        completed_slices.push(completed_entry);
                    }
                    ReconcileCohortMemberResult::Escalated {
                        slice_id,
                        worktree_path,
                        imports,
                        queue_sync,
                    } => {
                        apply_imports(&workspace_root, Path::new(&worktree_path), &imports)?;
                        sync_queue_patch(&queue_path, &slice_id, &queue_sync)?;
                        return Ok(ApplyCohortDispatchResult::Escalated {
                            slice_id,
                            worktree_path,
                            completed_count: completed_slices.len(),
                            completion_markers: completion_markers(&completed_slices),
                            completed_slices,
                            terminal: run_output.clone(),
                            stage: None,
                            stop_condition: None,
                            conflicting_slice_id: None,
                            conflicting_path: None,
                        });
                    }
                    ReconcileCohortMemberResult::MergeConflict {
                        slice_id,
                        worktree_path,
                        imports,
                        queue_sync,
                        conflicting_slice_id,
                        conflicting_path,
                    } => {
                        apply_imports(&workspace_root, Path::new(&worktree_path), &imports)?;
                        sync_queue_patch(&queue_path, &slice_id, &queue_sync)?;
                        return Ok(ApplyCohortDispatchResult::Escalated {
                            slice_id,
                            worktree_path,
                            completed_count: completed_slices.len(),
                            completion_markers: completion_markers(&completed_slices),
                            completed_slices,
                            terminal: run_output.clone(),
                            stage: Some("cohort_merge".to_string()),
                            stop_condition: Some("merge_conflict".to_string()),
                            conflicting_slice_id: Some(conflicting_slice_id),
                            conflicting_path: Some(conflicting_path),
                        });
                    }
                }
            }
        }
    }

    Ok(ApplyCohortDispatchResult::Completed {
        completed_count: completed_slices.len(),
        completion_markers: completion_markers(&completed_slices),
        completed_slices,
    })
}

fn parse_dispatched_members(raw_members: &[String]) -> Result<Vec<DispatchedCohortMember>> {
    raw_members
        .iter()
        .map(|raw| {
            serde_json::from_str::<DispatchedCohortMember>(raw)
                .with_context(|| format!("failed to parse dispatched cohort member JSON `{raw}`"))
        })
        .collect()
}

fn merged_path_owner_args(owners: &HashMap<String, String>) -> Vec<String> {
    owners
        .iter()
        .map(|(path, slice_id)| format!("{path}={slice_id}"))
        .collect()
}

fn apply_imports(
    workspace_root: &Path,
    worktree_root: &Path,
    imports: &[crate::cohort_reconcile::ImportEntry],
) -> Result<()> {
    for import in imports {
        let workspace_path = workspace_root.join(&import.path);

        if import.status == "D" {
            match fs::remove_file(&workspace_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("failed to remove {}", display_path(&workspace_path))
                    });
                }
            }
            continue;
        }

        let source_path = worktree_root.join(&import.path);
        if let Some(parent) = workspace_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", display_path(parent)))?;
        }

        fs::copy(&source_path, &workspace_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                display_path(&source_path),
                display_path(&workspace_path)
            )
        })?;
    }

    Ok(())
}

fn sync_queue_patch(queue_path: &Path, slice_id: &str, patch: &QueueSyncPatch) -> Result<()> {
    update_slice(UpdateSliceOptions {
        queue_path: queue_path.to_path_buf(),
        slice_id: slice_id.to_string(),
        status: Some(patch.status),
        attempts: Some(patch.attempts),
        micro_corrections_used: Some(patch.micro_corrections_used),
        karai_structural: patch.karai_structural,
        bishop: patch.bishop,
        tiger_claw: patch.tiger_claw,
        micro_correction: patch.micro_correction,
        completed_at: patch.completed_at.clone(),
        clear_completed_at: false,
        escalation_reason: patch.escalation_reason.clone(),
        clear_escalation_reason: false,
    })?;

    Ok(())
}

fn append_dispatch_log_entry(
    workspace_root: &Path,
    dispatch_log_path: &Path,
    worktree_root: &Path,
    slice_id: &str,
) -> Result<()> {
    let worktree_log_path = worktree_root.join(".mutagen/state/dispatch-log.jsonl");
    if !worktree_log_path.is_file() {
        return Ok(());
    }

    let worktree_log = fs::read_to_string(&worktree_log_path)
        .with_context(|| format!("failed to read {}", display_path(&worktree_log_path)))?;
    let needle = format!("\"slice_id\":\"{slice_id}\"");
    let Some(log_entry) = worktree_log
        .lines()
        .rev()
        .find(|line| line.contains(&needle))
        .map(str::to_string)
    else {
        return Ok(());
    };

    let existing_log = fs::read_to_string(dispatch_log_path).unwrap_or_default();
    if existing_log.lines().any(|line| line.contains(&needle)) {
        return Ok(());
    }

    if let Some(parent) = dispatch_log_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", display_path(parent)))?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dispatch_log_path)
        .with_context(|| format!("failed to open {}", display_path(dispatch_log_path)))?;
    writeln!(file, "{log_entry}")
        .with_context(|| format!("failed to append {}", display_path(dispatch_log_path)))?;

    let _ = workspace_root;
    Ok(())
}

fn record_merged_paths(
    owners: &mut HashMap<String, String>,
    slice_id: &str,
    imports: &[crate::cohort_reconcile::ImportEntry],
) {
    for import in imports {
        owners.insert(import.path.clone(), slice_id.to_string());
    }
}

fn completion_markers(completed_slices: &[CompletedEntry]) -> Vec<String> {
    completed_slices
        .iter()
        .map(|entry| entry.completion_marker.clone())
        .collect()
}

fn resolve_workspace_root(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("missing workspace path");
    }

    if path.exists() {
        fs::canonicalize(path).with_context(|| format!("failed to resolve {}", display_path(path)))
    } else if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("failed to read current working directory")?
            .join(path))
    }
}

fn resolve_workspace_path(workspace_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
