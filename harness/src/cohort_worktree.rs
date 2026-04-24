use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct MaterializeCohortWorktreesOptions {
    pub workspace_root: PathBuf,
    pub slice_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CohortWorktreeMember {
    pub slice_id: String,
    pub worktree_path: String,
    pub result_path: String,
    pub status_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MaterializeCohortWorktreesResult {
    pub workspace_root: String,
    pub run_id: String,
    pub worktree_root: String,
    pub members: Vec<CohortWorktreeMember>,
}

#[derive(Debug, Clone)]
pub struct CleanupCohortWorktreesOptions {
    pub workspace_root: PathBuf,
    pub worktree_root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupCohortWorktreesResult {
    pub workspace_root: String,
    pub worktree_root: String,
    pub removed_worktrees: Vec<String>,
    pub removed_worktree_root: bool,
}

pub fn materialize_cohort_worktrees(
    options: MaterializeCohortWorktreesOptions,
) -> Result<MaterializeCohortWorktreesResult> {
    let workspace_root = resolve_workspace_root(&options.workspace_root)?;

    if options.slice_ids.is_empty() {
        bail!("at least one `slice_id` is required");
    }

    ensure_git_worktree_capable(&workspace_root)?;

    let run_id = allocate_run_id()?;
    let worktree_root = workspace_root.join(".mutagen/worktrees").join(&run_id);
    fs::create_dir_all(&worktree_root)
        .with_context(|| format!("failed to create {}", display_path(&worktree_root)))?;

    let mut members = Vec::new();
    let mut created_worktrees = Vec::new();

    for slice_id in &options.slice_ids {
        let worktree_path = worktree_root.join(slice_id);
        let result_path = worktree_root.join(format!("{slice_id}.result"));
        let status_path = worktree_root.join(format!("{slice_id}.exit"));

        if let Err(error) = materialize_member(&workspace_root, &worktree_path) {
            cleanup_materialized_worktrees(&workspace_root, &worktree_root, &created_worktrees);
            return Err(error);
        }

        created_worktrees.push(worktree_path.clone());
        members.push(CohortWorktreeMember {
            slice_id: slice_id.clone(),
            worktree_path: display_path(&worktree_path),
            result_path: display_path(&result_path),
            status_path: display_path(&status_path),
        });
    }

    Ok(MaterializeCohortWorktreesResult {
        workspace_root: display_path(&workspace_root),
        run_id,
        worktree_root: display_path(&worktree_root),
        members,
    })
}

pub fn cleanup_cohort_worktrees(
    options: CleanupCohortWorktreesOptions,
) -> Result<CleanupCohortWorktreesResult> {
    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let worktree_root = resolve_workspace_path(&workspace_root, &options.worktree_root);
    ensure_worktree_root_is_safe(&workspace_root, &worktree_root)?;

    let managed_worktrees = managed_worktrees_under_root(&workspace_root, &worktree_root)?;
    let mut removed_worktrees = Vec::new();

    for worktree_path in &managed_worktrees {
        if remove_worktree_safe(&workspace_root, worktree_path)? {
            removed_worktrees.push(display_path(worktree_path));
        }
    }

    let removed_worktree_root = if worktree_root.exists() {
        fs::remove_dir_all(&worktree_root)
            .with_context(|| format!("failed to remove {}", display_path(&worktree_root)))?;
        true
    } else {
        false
    };

    prune_worktrees(&workspace_root)?;

    Ok(CleanupCohortWorktreesResult {
        workspace_root: display_path(&workspace_root),
        worktree_root: display_path(&worktree_root),
        removed_worktrees,
        removed_worktree_root,
    })
}

fn materialize_member(workspace_root: &Path, worktree_path: &Path) -> Result<()> {
    add_worktree(workspace_root, worktree_path)?;
    if let Err(error) = snapshot_workspace_into_worktree(workspace_root, worktree_path) {
        let _ = remove_worktree_safe(workspace_root, worktree_path);
        return Err(error);
    }

    Ok(())
}

fn add_worktree(workspace_root: &Path, worktree_path: &Path) -> Result<()> {
    run_git(
        workspace_root,
        &["worktree", "add", "--detach", &display_path(worktree_path)],
    )
    .map(|_| ())
}

fn snapshot_workspace_into_worktree(workspace_root: &Path, worktree_path: &Path) -> Result<()> {
    copy_tree_recursive(
        workspace_root,
        workspace_root,
        worktree_path,
        &[Path::new(".git"), Path::new(".mutagen/worktrees")],
    )
}

fn copy_tree_recursive(
    root_source: &Path,
    current_source: &Path,
    destination: &Path,
    excluded: &[&Path],
) -> Result<()> {
    for entry in fs::read_dir(current_source)
        .with_context(|| format!("failed to read {}", display_path(current_source)))?
    {
        let entry = entry
            .with_context(|| format!("failed to read entry in {}", display_path(current_source)))?;
        let source_path = entry.path();
        let relative_path = source_path.strip_prefix(root_source).with_context(|| {
            format!(
                "failed to strip {} from {}",
                display_path(root_source),
                display_path(&source_path)
            )
        })?;

        if should_exclude(relative_path, excluded) {
            continue;
        }

        let destination_path = destination.join(relative_path);
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to stat {}", display_path(&source_path)))?;

        if file_type.is_dir() {
            fs::create_dir_all(&destination_path)
                .with_context(|| format!("failed to create {}", display_path(&destination_path)))?;
            copy_tree_recursive(root_source, &source_path, destination, excluded)?;
            continue;
        }

        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", display_path(parent)))?;
        }

        fs::copy(&source_path, &destination_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                display_path(&source_path),
                display_path(&destination_path)
            )
        })?;
    }

    Ok(())
}

fn should_exclude(relative_path: &Path, excluded: &[&Path]) -> bool {
    excluded
        .iter()
        .any(|prefix| relative_path.starts_with(prefix))
}

fn cleanup_materialized_worktrees(
    workspace_root: &Path,
    worktree_root: &Path,
    created_worktrees: &[PathBuf],
) {
    for worktree_path in created_worktrees {
        let _ = remove_worktree_safe(workspace_root, worktree_path);
    }

    let _ = fs::remove_dir_all(worktree_root);
    let _ = prune_worktrees(workspace_root);
}

fn ensure_git_worktree_capable(workspace_root: &Path) -> Result<()> {
    run_git(workspace_root, &["rev-parse", "--is-inside-work-tree"])
        .context("bounded cohort execution requires a git worktree-capable repository")?;
    Ok(())
}

fn managed_worktrees_under_root(
    workspace_root: &Path,
    worktree_root: &Path,
) -> Result<Vec<PathBuf>> {
    let output = run_git(workspace_root, &["worktree", "list", "--porcelain"])?;
    let mut worktrees = Vec::new();

    for line in String::from_utf8(output.stdout)
        .context("git worktree list output was not valid UTF-8")?
        .lines()
    {
        if let Some(path) = line.strip_prefix("worktree ") {
            let candidate = PathBuf::from(path);
            if candidate.starts_with(worktree_root) {
                worktrees.push(candidate);
            }
        }
    }

    Ok(worktrees)
}

fn remove_worktree_safe(workspace_root: &Path, worktree_path: &Path) -> Result<bool> {
    let managed = managed_worktrees_under_root(
        workspace_root,
        worktree_path.parent().unwrap_or(workspace_root),
    )?;
    if !managed.iter().any(|candidate| candidate == worktree_path) {
        return Ok(false);
    }

    run_git(
        workspace_root,
        &[
            "worktree",
            "remove",
            "--force",
            &display_path(worktree_path),
        ],
    )?;
    Ok(true)
}

fn prune_worktrees(workspace_root: &Path) -> Result<()> {
    run_git(workspace_root, &["worktree", "prune"]).map(|_| ())
}

fn ensure_worktree_root_is_safe(workspace_root: &Path, worktree_root: &Path) -> Result<()> {
    let expected_prefix = workspace_root.join(".mutagen").join("worktrees");
    if !worktree_root.starts_with(&expected_prefix) {
        bail!(
            "worktree root {} is outside {}",
            display_path(worktree_root),
            display_path(&expected_prefix)
        );
    }
    Ok(())
}

fn run_git(workspace_root: &Path, args: &[&str]) -> Result<std::process::Output> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace_root)
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

fn allocate_run_id() -> Result<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?
        .as_nanos();
    Ok(format!("{}-{}", std::process::id(), nanos))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_mutagen_worktrees_and_git_root() {
        assert!(should_exclude(
            Path::new(".git"),
            &[Path::new(".git"), Path::new(".mutagen/worktrees")]
        ));
        assert!(should_exclude(
            Path::new(".mutagen/worktrees/run-1"),
            &[Path::new(".git"), Path::new(".mutagen/worktrees")]
        ));
        assert!(!should_exclude(
            Path::new(".mutagen/state/active-slice.json"),
            &[Path::new(".git"), Path::new(".mutagen/worktrees")]
        ));
    }

    #[test]
    fn cleanup_rejects_roots_outside_managed_prefix() {
        let workspace = PathBuf::from("/repo");
        let candidate = PathBuf::from("/repo/not-managed");
        let error = ensure_worktree_root_is_safe(&workspace, &candidate)
            .expect_err("unsafe roots should be rejected");
        assert!(error.to_string().contains("outside"));
    }
}
