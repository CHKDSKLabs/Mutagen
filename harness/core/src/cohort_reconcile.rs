use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::policy::{dedupe_globs, default_author_write_set};
use crate::queue::{BishopVerdict, KaraiStructuralVerdict, Slice, SliceStatus, TigerClawVerdict};
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct ReconcileCohortMemberOptions {
    pub workspace_root: PathBuf,
    pub worktree_root: PathBuf,
    pub slice_id: String,
    pub run_output_path: PathBuf,
    pub merged_path_owners: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportEntry {
    pub status: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueSyncPatch {
    pub status: SliceStatus,
    pub attempts: u32,
    pub micro_corrections_used: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub karai_structural: Option<KaraiStructuralVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bishop: Option<BishopVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiger_claw: Option<TigerClawVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub micro_correction: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub escalation_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletedEntry {
    pub slice_id: String,
    pub completion_marker: String,
    pub review_skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_path: Option<String>,
    pub worktree_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReconcileCohortMemberResult {
    Completed {
        slice_id: String,
        worktree_path: String,
        imports: Vec<ImportEntry>,
        queue_sync: QueueSyncPatch,
        completed_entry: CompletedEntry,
        author_output_path: String,
    },
    Escalated {
        slice_id: String,
        worktree_path: String,
        imports: Vec<ImportEntry>,
        queue_sync: QueueSyncPatch,
    },
    MergeConflict {
        slice_id: String,
        worktree_path: String,
        imports: Vec<ImportEntry>,
        queue_sync: QueueSyncPatch,
        conflicting_slice_id: String,
        conflicting_path: String,
    },
}

#[derive(Debug, Clone)]
struct DeltaEntry {
    status: String,
    path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemberStatus {
    Completed,
    Escalated,
}

pub fn reconcile_cohort_member(
    options: ReconcileCohortMemberOptions,
) -> Result<ReconcileCohortMemberResult> {
    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let worktree_root = resolve_workspace_root(&options.worktree_root)?;
    let worktree_queue_path = worktree_root.join("slices/queue.json");
    let run_output_path = resolve_workspace_path(&worktree_root, &options.run_output_path);

    let run_output = read_run_output(&run_output_path)?;
    let member_status = member_status(&run_output)?;

    let queue = load_queue_file(&worktree_queue_path)?;
    let slice = queue
        .slices
        .iter()
        .find(|candidate| candidate.id == options.slice_id)
        .cloned()
        .with_context(|| format!("slice `{}` not found in worktree queue", options.slice_id))?;

    let merged_path_owners = parse_merged_path_owners(&options.merged_path_owners)?;
    let delta_entries = collect_delta_entries(&worktree_root)?;

    match member_status {
        MemberStatus::Completed => reconcile_completed(
            &workspace_root,
            &worktree_root,
            &slice,
            &run_output,
            &delta_entries,
            &merged_path_owners,
        ),
        MemberStatus::Escalated => Ok(ReconcileCohortMemberResult::Escalated {
            slice_id: slice.id.clone(),
            worktree_path: display_path(&worktree_root),
            imports: build_import_entries(
                &workspace_root,
                &worktree_root,
                &slice,
                &delta_entries,
                ImportMode::Diagnostics,
            )?,
            queue_sync: queue_sync_patch_for(&slice, None),
        }),
    }
}

fn reconcile_completed(
    workspace_root: &Path,
    worktree_root: &Path,
    slice: &Slice,
    run_output: &Value,
    delta_entries: &[DeltaEntry],
    merged_path_owners: &HashMap<String, String>,
) -> Result<ReconcileCohortMemberResult> {
    let imports = build_import_entries(
        workspace_root,
        worktree_root,
        slice,
        delta_entries,
        ImportMode::Completed,
    )?;

    if let Some((conflicting_path, conflicting_slice_id)) =
        first_merge_conflict(&imports, merged_path_owners)
    {
        return Ok(ReconcileCohortMemberResult::MergeConflict {
            slice_id: slice.id.clone(),
            worktree_path: display_path(worktree_root),
            imports: build_import_entries(
                workspace_root,
                worktree_root,
                slice,
                delta_entries,
                ImportMode::Diagnostics,
            )?,
            queue_sync: queue_sync_patch_for(
                slice,
                Some(format!(
                    "cohort merge conflict on {} with {}",
                    conflicting_path, conflicting_slice_id
                )),
            ),
            conflicting_slice_id,
            conflicting_path,
        });
    }

    let author_output_path = worktree_root
        .join(".mutagen/state/author-output")
        .join(format!("{}.md", safe_file_name(&slice.id)));

    Ok(ReconcileCohortMemberResult::Completed {
        slice_id: slice.id.clone(),
        worktree_path: display_path(worktree_root),
        imports,
        queue_sync: queue_sync_patch_for(slice, None),
        completed_entry: completed_entry_for(run_output, worktree_root)?,
        author_output_path: display_path(&author_output_path),
    })
}

fn completed_entry_for(run_output: &Value, worktree_root: &Path) -> Result<CompletedEntry> {
    let slice_id = required_string(run_output, "slice_id")?;
    let completion_marker = optional_string(
        run_output
            .get("finalize")
            .and_then(|value| value.get("completion_marker")),
    )
    .unwrap_or_default();
    let review_skipped = run_output
        .get("review_skipped")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let summary_path = optional_string(
        run_output
            .get("finalize")
            .and_then(|value| value.get("summary_path")),
    );

    Ok(CompletedEntry {
        slice_id,
        completion_marker,
        review_skipped,
        summary_path,
        worktree_path: display_path(worktree_root),
    })
}

fn queue_sync_patch_for(slice: &Slice, escalation_reason: Option<String>) -> QueueSyncPatch {
    QueueSyncPatch {
        status: escalation_reason
            .as_ref()
            .map(|_| SliceStatus::Escalated)
            .unwrap_or(slice.status),
        attempts: slice.attempts,
        micro_corrections_used: slice.micro_corrections_used,
        karai_structural: slice.verdicts.karai_structural,
        bishop: slice.verdicts.bishop,
        tiger_claw: slice.verdicts.tiger_claw,
        micro_correction: slice.verdicts.micro_correction,
        completed_at: slice.completed_at.clone(),
        escalation_reason: escalation_reason.or_else(|| slice.escalation_reason.clone()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportMode {
    Completed,
    Diagnostics,
}

fn build_import_entries(
    workspace_root: &Path,
    worktree_root: &Path,
    slice: &Slice,
    delta_entries: &[DeltaEntry],
    mode: ImportMode,
) -> Result<Vec<ImportEntry>> {
    let mut imports = Vec::new();
    let selection_scope = selection_scope_for(slice)?;
    let safe_slice_id = safe_file_name(&slice.id);

    for delta in delta_entries {
        if path_is_regenerable_import(&delta.path) {
            continue;
        }

        let allowed = match mode {
            ImportMode::Completed => {
                path_allowed_for_completed(&delta.path, &slice.id, &safe_slice_id, &selection_scope)
            }
            ImportMode::Diagnostics => {
                path_allowed_for_diagnostics(&delta.path, &slice.id, &safe_slice_id)
            }
        };

        if !allowed {
            continue;
        }

        if !path_differs_from_main(workspace_root, worktree_root, delta)? {
            continue;
        }

        imports.push(ImportEntry {
            status: delta.status.clone(),
            path: delta.path.clone(),
        });
    }

    Ok(imports)
}

fn selection_scope_for(slice: &Slice) -> Result<Vec<String>> {
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

fn path_allowed_for_completed(
    path: &str,
    slice_id: &str,
    safe_slice_id: &str,
    selection_scope: &[String],
) -> bool {
    if selection_scope
        .iter()
        .any(|glob| path_matches_glob(path, glob))
    {
        return true;
    }

    matches!(
        path,
        _ if path.starts_with(&format!("reviews/{slice_id}/"))
            || path.starts_with(&format!("slices/{slice_id}/"))
            || path == format!(".mutagen/state/author-output/{safe_slice_id}.md")
            || path == format!(".mutagen/state/review-output/{safe_slice_id}.md")
            || path == format!(".mutagen/state/evidence/{safe_slice_id}.md")
            || path == ".mutagen/state/tiger-claw-latest.md"
            || path.starts_with(&format!(".mutagen/state/dispatch/{slice_id}/"))
            || path.starts_with("tests/qa/")
    )
}

fn path_allowed_for_diagnostics(path: &str, slice_id: &str, safe_slice_id: &str) -> bool {
    path.starts_with(&format!("reviews/{slice_id}/"))
        || path == format!(".mutagen/state/author-output/{safe_slice_id}.md")
        || path == format!(".mutagen/state/review-output/{safe_slice_id}.md")
        || path == format!(".mutagen/state/evidence/{safe_slice_id}.md")
        || path == ".mutagen/state/tiger-claw-latest.md"
        || path.starts_with(&format!(".mutagen/state/dispatch/{slice_id}/"))
}

fn path_matches_glob(path: &str, glob: &str) -> bool {
    globset::GlobBuilder::new(glob)
        .literal_separator(true)
        .build()
        .map(|compiled| compiled.compile_matcher().is_match(path))
        .unwrap_or(false)
}

fn path_differs_from_main(
    workspace_root: &Path,
    worktree_root: &Path,
    delta: &DeltaEntry,
) -> Result<bool> {
    let main_path = workspace_root.join(&delta.path);
    let worktree_path = worktree_root.join(&delta.path);

    if delta.status == "D" {
        return Ok(main_path.exists());
    }

    if !worktree_path.exists() {
        return Ok(false);
    }

    if !main_path.exists() {
        return Ok(true);
    }

    let main_bytes = fs::read(&main_path)
        .with_context(|| format!("failed to read {}", display_path(&main_path)))?;
    let worktree_bytes = fs::read(&worktree_path)
        .with_context(|| format!("failed to read {}", display_path(&worktree_path)))?;

    Ok(main_bytes != worktree_bytes)
}

fn first_merge_conflict(
    imports: &[ImportEntry],
    merged_path_owners: &HashMap<String, String>,
) -> Option<(String, String)> {
    for import in imports {
        if path_allows_shared_import(&import.path) || path_is_regenerable_import(&import.path) {
            continue;
        }

        if let Some(conflicting_slice_id) = merged_path_owners.get(&import.path) {
            return Some((import.path.clone(), conflicting_slice_id.clone()));
        }
    }

    None
}

fn path_allows_shared_import(path: &str) -> bool {
    path == ".mutagen/state/tiger-claw-latest.md"
}

fn path_is_regenerable_import(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    normalized.starts_with("tests/qa/") && normalized.ends_with("/Cargo.lock")
}

fn parse_merged_path_owners(entries: &[String]) -> Result<HashMap<String, String>> {
    let mut owners = HashMap::new();

    for entry in entries {
        let (path, slice_id) = entry
            .split_once('=')
            .with_context(|| format!("invalid merged-path owner entry `{entry}`"))?;
        if path.trim().is_empty() || slice_id.trim().is_empty() {
            bail!("invalid merged-path owner entry `{entry}`");
        }

        owners.insert(path.to_string(), slice_id.to_string());
    }

    Ok(owners)
}

fn collect_delta_entries(worktree_root: &Path) -> Result<Vec<DeltaEntry>> {
    let mut entries = collect_name_status_entries(
        worktree_root,
        &[
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            "--diff-filter=ADM",
            "--relative",
            "HEAD",
            "--",
        ],
    )?;

    let others = collect_null_terminated_entries(
        worktree_root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;

    entries.extend(others.into_iter().map(|path| DeltaEntry {
        status: "A".to_string(),
        path,
    }));

    Ok(entries)
}

fn collect_name_status_entries(worktree_root: &Path, args: &[&str]) -> Result<Vec<DeltaEntry>> {
    let output = run_git(worktree_root, args)?;
    let mut fields = output.stdout.split(|byte| *byte == 0);
    let mut entries = Vec::new();

    while let Some(status_bytes) = fields.next() {
        if status_bytes.is_empty() {
            break;
        }

        let Some(path_bytes) = fields.next() else {
            bail!("git output was missing a path after a status entry");
        };

        entries.push(DeltaEntry {
            status: String::from_utf8(status_bytes.to_vec())
                .context("git status entry was not valid UTF-8")?,
            path: String::from_utf8(path_bytes.to_vec())
                .context("git path entry was not valid UTF-8")?,
        });
    }

    Ok(entries)
}

fn collect_null_terminated_entries(worktree_root: &Path, args: &[&str]) -> Result<Vec<String>> {
    let output = run_git(worktree_root, args)?;
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            String::from_utf8(entry.to_vec()).context("git path entry was not valid UTF-8")
        })
        .collect()
}

fn run_git(worktree_root: &Path, args: &[&str]) -> Result<std::process::Output> {
    let output = Command::new("git")
        .args(args)
        .current_dir(worktree_root)
        .output()
        .with_context(|| format!("failed to execute git {}", args.join(" ")))?;

    if !output.status.success() {
        bail!(
            "git {} failed:\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(output)
}

fn read_run_output(path: &Path) -> Result<Value> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", display_path(path)))?;
    serde_json::from_str(&raw).with_context(|| {
        format!(
            "failed to parse run output JSON from {}",
            display_path(path)
        )
    })
}

fn member_status(run_output: &Value) -> Result<MemberStatus> {
    match required_string(run_output, "status")?.as_str() {
        "completed" => Ok(MemberStatus::Completed),
        "escalated" => Ok(MemberStatus::Escalated),
        other => bail!("unsupported cohort member status `{other}`"),
    }
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    optional_string(value.get(key)).with_context(|| format!("missing `{key}`"))
}

fn optional_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::{HumanCheckNeeded, SliceVerdicts, TraceSet, VerificationSteps};

    fn sample_slice() -> Slice {
        Slice {
            id: "L2-orders-002".to_string(),
            title: "Expose order creation API".to_string(),
            phase: Some("phase_1".to_string()),
            status: SliceStatus::Completed,
            author_agent: "Bebop".to_string(),
            layer: 1,
            bounded_context: "orders".to_string(),
            target_loc: 300,
            objective: "Expose the aggregate through an HTTP API.".to_string(),
            context_to_update: "project_state.md".to_string(),
            implementation_details: Vec::new(),
            review_required: true,
            attempts: 0,
            micro_corrections_used: 0,
            depends_on: Vec::new(),
            adjacent_scope_allowed: Vec::new(),
            write_set: vec!["src/http/**".to_string(), "tests/http/**".to_string()],
            traces_to: TraceSet::default(),
            verification_steps: VerificationSteps::default(),
            human_check_needed: HumanCheckNeeded::default(),
            verdicts: SliceVerdicts::default(),
            completed_at: Some("2026-04-23T18:00:00Z".to_string()),
            escalation_reason: None,
        }
    }

    #[test]
    fn completed_imports_allow_review_artifacts_outside_selection_scope() {
        let slice = sample_slice();
        let selection_scope = selection_scope_for(&slice).expect("selection scope should resolve");
        assert!(path_allowed_for_completed(
            "reviews/L2-orders-002/tiger-claw.md",
            &slice.id,
            &safe_file_name(&slice.id),
            &selection_scope
        ));
        assert!(path_allowed_for_completed(
            "tests/qa/shared.qa.test.rs",
            &slice.id,
            &safe_file_name(&slice.id),
            &selection_scope
        ));
        assert!(!path_allowed_for_completed(
            "src/orders/aggregate.rs",
            &slice.id,
            &safe_file_name(&slice.id),
            &selection_scope
        ));
    }

    #[test]
    fn merge_conflict_detection_ignores_shared_imports() {
        let imports = vec![
            ImportEntry {
                status: "M".to_string(),
                path: ".mutagen/state/tiger-claw-latest.md".to_string(),
            },
            ImportEntry {
                status: "A".to_string(),
                path: "tests/qa/shared.qa.test.rs".to_string(),
            },
        ];
        let owners = HashMap::from([
            (
                ".mutagen/state/tiger-claw-latest.md".to_string(),
                "L1-orders-001".to_string(),
            ),
            (
                "tests/qa/shared.qa.test.rs".to_string(),
                "L1-orders-001".to_string(),
            ),
        ]);

        let conflict = first_merge_conflict(&imports, &owners).expect("conflict should be found");
        assert_eq!(conflict.0, "tests/qa/shared.qa.test.rs");
        assert_eq!(conflict.1, "L1-orders-001");
    }

    #[test]
    fn merge_conflict_detection_ignores_regenerable_qa_lockfiles() {
        let imports = vec![ImportEntry {
            status: "A".to_string(),
            path: "tests/qa/service/Cargo.lock".to_string(),
        }];
        let owners = HashMap::from([(
            "tests/qa/service/Cargo.lock".to_string(),
            "L1-orders-001".to_string(),
        )]);

        assert!(first_merge_conflict(&imports, &owners).is_none());
    }
}
