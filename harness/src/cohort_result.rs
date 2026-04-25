use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CollectCohortMemberResultOptions {
    pub workspace_root: PathBuf,
    pub worktree_root: PathBuf,
    pub slice_id: String,
    pub result_path: PathBuf,
    pub status_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CollectCohortMemberResult {
    Ready {
        slice_id: String,
        worktree_path: String,
        member_status: String,
        run_output: Value,
    },
    Failed {
        slice_id: String,
        worktree_path: String,
        exit_status: String,
        message: String,
    },
}

pub fn collect_cohort_member_result(
    options: CollectCohortMemberResultOptions,
) -> Result<CollectCohortMemberResult> {
    if options.slice_id.trim().is_empty() {
        bail!("missing `slice_id`");
    }

    let _workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let worktree_root = resolve_workspace_root(&options.worktree_root)?;
    let result_path = resolve_workspace_path(&worktree_root, &options.result_path);
    let status_path = resolve_workspace_path(&worktree_root, &options.status_path);

    let exit_status = read_exit_status(&status_path)?;
    let raw_output = fs::read_to_string(&result_path)
        .with_context(|| format!("failed to read {}", display_path(&result_path)))?;

    if exit_status != "0" {
        return Ok(CollectCohortMemberResult::Failed {
            slice_id: options.slice_id,
            worktree_path: display_path(&worktree_root),
            exit_status,
            message: raw_output,
        });
    }

    let run_output: Value = serde_json::from_str(&raw_output).with_context(|| {
        format!(
            "failed to parse run output JSON from {}",
            display_path(&result_path)
        )
    })?;
    let member_status = required_string(&run_output, "status")?;

    match member_status.as_str() {
        "completed" | "escalated" => {}
        other => bail!("unsupported cohort member status `{other}`"),
    }

    Ok(CollectCohortMemberResult::Ready {
        slice_id: options.slice_id,
        worktree_path: display_path(&worktree_root),
        member_status,
        run_output,
    })
}

fn read_exit_status(path: &Path) -> Result<String> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", display_path(path)))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!("missing exit status in {}", display_path(path));
    }

    if !trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        bail!("invalid exit status `{trimmed}` in {}", display_path(path));
    }

    Ok(trimmed.to_string())
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .with_context(|| format!("missing `{key}`"))
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
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reads_failed_member_output_verbatim() {
        let fixture = TempFixture::new("failed");
        fixture.write("member.result", "loud and broken\n");
        fixture.write("member.exit", "7\n");

        let result = collect_cohort_member_result(CollectCohortMemberResultOptions {
            workspace_root: fixture.root.clone(),
            worktree_root: fixture.root.clone(),
            slice_id: "L1-orders-001".to_string(),
            result_path: PathBuf::from("member.result"),
            status_path: PathBuf::from("member.exit"),
        })
        .expect("failed member should still collect");

        match result {
            CollectCohortMemberResult::Failed {
                slice_id,
                exit_status,
                message,
                ..
            } => {
                assert_eq!(slice_id, "L1-orders-001");
                assert_eq!(exit_status, "7");
                assert_eq!(message, "loud and broken\n");
            }
            other => panic!("expected failed result, got {other:?}"),
        }
    }

    #[test]
    fn parses_ready_member_json() {
        let fixture = TempFixture::new("ready");
        fixture.write(
            "member.result",
            "{\n  \"status\": \"completed\",\n  \"slice_id\": \"L1-orders-001\"\n}\n",
        );
        fixture.write("member.exit", "0\n");

        let result = collect_cohort_member_result(CollectCohortMemberResultOptions {
            workspace_root: fixture.root.clone(),
            worktree_root: fixture.root.clone(),
            slice_id: "L1-orders-001".to_string(),
            result_path: PathBuf::from("member.result"),
            status_path: PathBuf::from("member.exit"),
        })
        .expect("ready member should parse");

        match result {
            CollectCohortMemberResult::Ready {
                member_status,
                run_output,
                ..
            } => {
                assert_eq!(member_status, "completed");
                assert_eq!(run_output["slice_id"], "L1-orders-001");
            }
            other => panic!("expected ready result, got {other:?}"),
        }
    }

    struct TempFixture {
        root: PathBuf,
    }

    impl TempFixture {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "mutagen-harness-cohort-result-{name}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("temp fixture should be created");
            Self { root }
        }

        fn write(&self, relative: &str, body: &str) {
            let path = self.root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("temp fixture parent should exist");
            }
            fs::write(path, body).expect("temp fixture file should write");
        }
    }

    impl Drop for TempFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
