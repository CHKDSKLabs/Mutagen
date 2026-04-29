use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::adapter::HostKind;
use crate::cohort_result::{
    CollectCohortMemberResult, CollectCohortMemberResultOptions, collect_cohort_member_result,
};

#[derive(Debug, Clone)]
pub struct DispatchCohortMembersOptions {
    pub workspace_root: PathBuf,
    pub runner_script_path: PathBuf,
    pub host: HostKind,
    pub member_json: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchedCohortMember {
    pub slice_id: String,
    pub worktree_path: String,
    pub result_path: String,
    pub status_path: String,
    pub outcome: CollectCohortMemberResult,
}

#[derive(Debug, Clone, Serialize)]
pub struct DispatchCohortMembersResult {
    pub workspace_root: String,
    pub host: HostKind,
    pub runner_script_path: String,
    pub members: Vec<DispatchedCohortMember>,
}

#[derive(Debug, Clone, Deserialize)]
struct DispatchMemberSpec {
    slice_id: String,
    worktree_path: String,
    result_path: String,
    status_path: String,
}

pub fn dispatch_cohort_members(
    options: DispatchCohortMembersOptions,
) -> Result<DispatchCohortMembersResult> {
    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let runner_script_path = resolve_workspace_path(&workspace_root, &options.runner_script_path);

    if options.member_json.is_empty() {
        bail!("at least one `member_json` entry is required");
    }

    if !runner_script_path.exists() {
        bail!(
            "runner script not found at {}",
            display_path(&runner_script_path)
        );
    }

    let member_specs = parse_member_specs(&options.member_json)?;
    let mut children = Vec::new();

    for spec in &member_specs {
        let result_path = resolve_workspace_path(&workspace_root, Path::new(&spec.result_path));
        let status_path = resolve_workspace_path(&workspace_root, Path::new(&spec.status_path));
        let worktree_path = resolve_workspace_path(&workspace_root, Path::new(&spec.worktree_path));

        if let Some(parent) = result_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", display_path(parent)))?;
        }

        if let Some(parent) = status_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", display_path(parent)))?;
        }

        let result_file = File::create(&result_path)
            .with_context(|| format!("failed to create {}", display_path(&result_path)))?;
        let stderr_file = result_file
            .try_clone()
            .with_context(|| format!("failed to clone {}", display_path(&result_path)))?;

        let child = Command::new("bash")
            .arg(display_path(&runner_script_path))
            .arg("--workspace-root")
            .arg(display_path(&worktree_path))
            .arg("--queue")
            .arg(display_path(&worktree_path.join("slices/queue.json")))
            .arg("--queue-validation")
            .arg(display_path(
                &worktree_path.join(".mutagen/state/queue-validation.json"),
            ))
            .arg("--workflow-config")
            .arg(display_path(&worktree_path.join(".claude/workflow.json")))
            .arg("--active-state")
            .arg(display_path(
                &worktree_path.join(".mutagen/state/active-slice.json"),
            ))
            .arg("--author-output-dir")
            .arg(display_path(
                &worktree_path.join(".mutagen/state/author-output"),
            ))
            .arg("--dispatch-root")
            .arg(display_path(&worktree_path.join(".mutagen/state/dispatch")))
            .arg("--dispatch-log")
            .arg(display_path(
                &worktree_path.join(".mutagen/state/dispatch-log.jsonl"),
            ))
            .arg("--summary-root")
            .arg(display_path(&worktree_path.join("slices")))
            .arg("--slicemap")
            .arg(display_path(&worktree_path.join("slices/slicemap.md")))
            .arg("--legacy")
            .arg(display_path(&worktree_path.join("slices/queue.md")))
            .arg("--host")
            .arg(host_kind_name(options.host))
            .arg("--slice-id")
            .arg(&spec.slice_id)
            .stdout(Stdio::from(result_file))
            .stderr(Stdio::from(stderr_file))
            .spawn()
            .with_context(|| {
                format!(
                    "failed to spawn cohort member `{}` with {}",
                    spec.slice_id,
                    display_path(&runner_script_path)
                )
            })?;

        children.push(DispatchChild {
            spec: spec.clone(),
            worktree_path,
            result_path,
            status_path,
            child,
        });
    }

    let mut members = Vec::new();

    for mut dispatch in children {
        let exit_code = dispatch
            .child
            .wait()
            .with_context(|| format!("failed to wait for `{}`", dispatch.spec.slice_id))?
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "1".to_string());

        fs::write(&dispatch.status_path, format!("{exit_code}\n"))
            .with_context(|| format!("failed to write {}", display_path(&dispatch.status_path)))?;

        let outcome = collect_cohort_member_result(CollectCohortMemberResultOptions {
            workspace_root: workspace_root.clone(),
            worktree_root: dispatch.worktree_path.clone(),
            slice_id: dispatch.spec.slice_id.clone(),
            result_path: dispatch.result_path.clone(),
            status_path: dispatch.status_path.clone(),
        })?;

        members.push(DispatchedCohortMember {
            slice_id: dispatch.spec.slice_id,
            worktree_path: display_path(&dispatch.worktree_path),
            result_path: display_path(&dispatch.result_path),
            status_path: display_path(&dispatch.status_path),
            outcome,
        });
    }

    Ok(DispatchCohortMembersResult {
        workspace_root: display_path(&workspace_root),
        host: options.host,
        runner_script_path: display_path(&runner_script_path),
        members,
    })
}

struct DispatchChild {
    spec: DispatchMemberSpec,
    worktree_path: PathBuf,
    result_path: PathBuf,
    status_path: PathBuf,
    child: std::process::Child,
}

fn parse_member_specs(raw_members: &[String]) -> Result<Vec<DispatchMemberSpec>> {
    raw_members
        .iter()
        .map(|raw| {
            serde_json::from_str::<DispatchMemberSpec>(raw)
                .with_context(|| format!("failed to parse cohort member JSON `{raw}`"))
        })
        .collect()
}

fn host_kind_name(host: HostKind) -> &'static str {
    match host {
        HostKind::Codex => "codex",
        HostKind::Claude => "claude",
        HostKind::Stub => "stub",
    }
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
    fn parses_member_specs_from_json() {
        let members = parse_member_specs(&[r#"{
            "slice_id": "L1-orders-001",
            "worktree_path": "/tmp/worktree/L1-orders-001",
            "result_path": "/tmp/worktree/L1-orders-001.result",
            "status_path": "/tmp/worktree/L1-orders-001.exit"
        }"#
        .to_string()])
        .expect("member JSON should parse");

        assert_eq!(members.len(), 1);
        assert_eq!(members[0].slice_id, "L1-orders-001");
    }
}
