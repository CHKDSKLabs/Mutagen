use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::queue::{BishopVerdict, SliceStatus, TigerClawVerdict};
use crate::state::{Stage, load_active_slice};
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct RecordReviewVerdictOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub active_state_path: PathBuf,
    pub qa_report_path: Option<PathBuf>,
    pub latest_qa_report_path: Option<PathBuf>,
    pub slice_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordReviewVerdictResult {
    pub slice_id: String,
    pub queue_path: String,
    pub qa_report_path: String,
    pub latest_qa_report_path: String,
    pub status: SliceStatus,
    pub bishop: BishopVerdict,
    pub tiger_claw: TigerClawVerdict,
}

pub fn record_review_verdict(
    options: RecordReviewVerdictOptions,
) -> Result<RecordReviewVerdictResult> {
    if options.slice_id.trim().is_empty() {
        bail!("missing `slice_id`");
    }

    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let queue_path = resolve_workspace_path(&workspace_root, &options.queue_path);
    let active_state_path = resolve_workspace_path(&workspace_root, &options.active_state_path);
    let qa_report_path = options
        .qa_report_path
        .as_deref()
        .map(|path| resolve_workspace_path(&workspace_root, path))
        .unwrap_or_else(|| {
            workspace_root
                .join("reviews")
                .join(&options.slice_id)
                .join("tiger-claw.md")
        });
    let latest_qa_report_path = options
        .latest_qa_report_path
        .as_deref()
        .map(|path| resolve_workspace_path(&workspace_root, path))
        .unwrap_or_else(|| workspace_root.join(".mutagen/state/tiger-claw-latest.md"));

    let mut queue = load_queue_file(&queue_path)?;
    let active_state = load_active_slice(&active_state_path)?;

    if active_state.slice_id != options.slice_id {
        bail!(
            "active slice mismatch: expected `{}`, found `{}`",
            options.slice_id,
            active_state.slice_id
        );
    }

    if active_state.stage != Stage::Review {
        bail!(
            "cannot record review verdict for slice `{}` while active stage is `{}`",
            options.slice_id,
            stage_name(active_state.stage)
        );
    }

    let qa_report = fs::read_to_string(&qa_report_path).with_context(|| {
        format!(
            "failed to read QA report at {}",
            display_path(&qa_report_path)
        )
    })?;
    let latest_qa_report = fs::read_to_string(&latest_qa_report_path).with_context(|| {
        format!(
            "failed to read latest QA report at {}",
            display_path(&latest_qa_report_path)
        )
    })?;

    if latest_qa_report.trim().is_empty() {
        bail!(
            "latest QA report at {} is empty",
            display_path(&latest_qa_report_path)
        );
    }

    let qa_verdict = parse_tiger_claw_verdict(&qa_report).with_context(|| {
        format!(
            "failed to parse Tiger Claw verdict from {}",
            display_path(&qa_report_path)
        )
    })?;
    let latest_verdict = parse_tiger_claw_verdict(&latest_qa_report).with_context(|| {
        format!(
            "failed to parse Tiger Claw verdict from {}",
            display_path(&latest_qa_report_path)
        )
    })?;

    if qa_verdict != latest_verdict {
        bail!(
            "Tiger Claw verdict mismatch between {} and {}",
            display_path(&qa_report_path),
            display_path(&latest_qa_report_path)
        );
    }

    let result = {
        let slice = queue
            .slice_mut(&options.slice_id)
            .with_context(|| format!("slice `{}` not found", options.slice_id))?;

        if slice.status != SliceStatus::InProgress {
            bail!(
                "cannot record review verdict for slice `{}` from status `{}`",
                slice.id,
                slice_status_name(slice.status)
            );
        }

        slice.verdicts.bishop = Some(BishopVerdict::Skip);
        slice.verdicts.tiger_claw = Some(qa_verdict);

        RecordReviewVerdictResult {
            slice_id: slice.id.clone(),
            queue_path: display_path(&queue_path),
            qa_report_path: display_path(&qa_report_path),
            latest_qa_report_path: display_path(&latest_qa_report_path),
            status: slice.status,
            bishop: BishopVerdict::Skip,
            tiger_claw: qa_verdict,
        }
    };

    write_json_file(&queue_path, &queue)?;

    Ok(result)
}

fn parse_tiger_claw_verdict(report: &str) -> Option<TigerClawVerdict> {
    let mut in_verdict_section = false;

    for line in report.lines() {
        let trimmed = line.trim();

        if !in_verdict_section {
            if trimmed.eq_ignore_ascii_case("#### Verdict") {
                in_verdict_section = true;
            }
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('#') {
            break;
        }

        if let Some(verdict) = classify_verdict_line(trimmed) {
            return Some(verdict);
        }
    }

    None
}

fn classify_verdict_line(line: &str) -> Option<TigerClawVerdict> {
    let normalized = line.trim_matches('*').trim().to_ascii_lowercase();

    if normalized.contains("defect") {
        return Some(TigerClawVerdict::Defect);
    }

    if normalized.contains("gap") {
        return Some(TigerClawVerdict::Gap);
    }

    if normalized.contains("clean") {
        return Some(TigerClawVerdict::Clean);
    }

    if normalized.contains("skip") {
        return Some(TigerClawVerdict::Skip);
    }

    None
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

fn resolve_workspace_root(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("missing `workspace_root`");
    }

    if path.exists() {
        fs::canonicalize(path)
            .with_context(|| format!("failed to resolve workspace root {}", display_path(path)))
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
    path.to_string_lossy().into_owned()
}

fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Author => "author",
        Stage::StructuralCheck => "structural_check",
        Stage::Review => "review",
        Stage::StateRecord => "state_record",
    }
}

fn slice_status_name(status: SliceStatus) -> &'static str {
    match status {
        SliceStatus::Pending => "pending",
        SliceStatus::InProgress => "in_progress",
        SliceStatus::BlockedRetry => "blocked_retry",
        SliceStatus::Completed => "completed",
        SliceStatus::Escalated => "escalated",
        SliceStatus::Refused => "refused",
    }
}
