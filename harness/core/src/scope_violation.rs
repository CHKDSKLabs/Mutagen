use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::notifications::{NotificationEvent, StopCondition, scope_violation_notification};
use crate::queue::SliceStatus;
use crate::state::load_active_slice;
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct ScopeViolationOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub active_state_path: PathBuf,
    pub violation_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeViolationRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    #[serde(default)]
    pub decision: String,
    #[serde(default)]
    pub class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_write_globs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeViolationResult {
    pub violation_path: String,
    pub queue_path: String,
    pub stop_condition: StopCondition,
    pub notifications: Vec<NotificationEvent>,
    pub escalation_reason: String,
    pub queue_updated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_update_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slice_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<SliceStatus>,
    pub violation: ScopeViolationRecord,
}

pub fn scope_violation(options: ScopeViolationOptions) -> Result<ScopeViolationResult> {
    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let queue_path = resolve_workspace_path(&workspace_root, &options.queue_path);
    let active_state_path = resolve_workspace_path(&workspace_root, &options.active_state_path);
    let violation_path = resolve_workspace_path(&workspace_root, &options.violation_path);

    let raw = fs::read_to_string(&violation_path).with_context(|| {
        format!(
            "failed to read scope violation report at {}",
            display_path(&violation_path)
        )
    })?;
    let mut violation: ScopeViolationRecord =
        serde_json::from_str(&raw).context("failed to parse scope violation report JSON")?;
    let active_state = load_active_slice(&active_state_path).ok();

    enrich_violation_from_active_state(&mut violation, active_state.as_ref());

    if violation.path.trim().is_empty() {
        violation.path = "<unknown>".to_string();
    }

    if violation.decision.trim().is_empty() {
        violation.decision = "deny".to_string();
    }

    if violation.class.trim().is_empty() {
        violation.class = "unknown".to_string();
    }

    write_json_file(&violation_path, &violation)?;

    let escalation_reason = format!(
        "Traag DENY on {} (class: {}) during stage {}. Agent: {}.",
        violation.path,
        violation.class,
        violation.stage.as_deref().unwrap_or("unknown"),
        violation.active_agent.as_deref().unwrap_or("unknown"),
    );

    let notifications = vec![scope_violation_notification(
        violation.slice_id.as_deref(),
        &violation.path,
        &violation.class,
        violation.stage.as_deref(),
        violation.active_agent.as_deref(),
    )];

    let (queue_updated, queue_update_note, status) = escalate_queue_slice(
        &queue_path,
        violation.slice_id.as_deref(),
        &escalation_reason,
    )?;

    Ok(ScopeViolationResult {
        violation_path: display_path(&violation_path),
        queue_path: display_path(&queue_path),
        stop_condition: StopCondition::ScopeViolation,
        notifications,
        escalation_reason,
        queue_updated,
        queue_update_note,
        slice_id: violation.slice_id.clone(),
        status,
        violation,
    })
}

fn enrich_violation_from_active_state(
    violation: &mut ScopeViolationRecord,
    active_state: Option<&crate::state::ActiveSliceState>,
) {
    let Some(active_state) = active_state else {
        return;
    };

    fill_if_missing(&mut violation.slice_id, Some(active_state.slice_id.clone()));
    fill_if_missing(&mut violation.title, Some(active_state.title.clone()));
    fill_if_missing(
        &mut violation.stage,
        Some(stage_name(active_state.stage).to_string()),
    );
    fill_if_missing(
        &mut violation.active_agent,
        Some(active_state.active_agent.clone()),
    );
    fill_if_missing(
        &mut violation.author_agent,
        Some(active_state.author_agent.clone()),
    );

    if violation.allowed_write_globs.is_empty() {
        violation.allowed_write_globs = active_state.allowed_write_globs.clone();
    }
}

fn fill_if_missing(target: &mut Option<String>, value: Option<String>) {
    if target
        .as_deref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
    {
        *target = value;
    }
}

fn escalate_queue_slice(
    queue_path: &Path,
    slice_id: Option<&str>,
    escalation_reason: &str,
) -> Result<(bool, Option<String>, Option<SliceStatus>)> {
    let Some(slice_id) = slice_id else {
        return Ok((
            false,
            Some("scope violation report does not name a slice".to_string()),
            None,
        ));
    };

    if !queue_path.exists() {
        return Ok((
            false,
            Some(format!(
                "queue file not found at {}",
                display_path(queue_path)
            )),
            None,
        ));
    }

    let mut queue = match load_queue_file(queue_path) {
        Ok(queue) => queue,
        Err(error) => {
            return Ok((
                false,
                Some(format!("queue could not be read: {error:#}")),
                None,
            ));
        }
    };

    let Some(slice) = queue.slice_mut(slice_id) else {
        return Ok((
            false,
            Some(format!("slice `{slice_id}` not found in queue")),
            None,
        ));
    };

    slice.status = SliceStatus::Escalated;
    slice.escalation_reason = Some(escalation_reason.to_string());
    let status = slice.status;

    write_json_file(queue_path, &queue)?;

    Ok((true, None, Some(status)))
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

fn stage_name(stage: crate::state::Stage) -> &'static str {
    match stage {
        crate::state::Stage::Author => "author",
        crate::state::Stage::StructuralCheck => "structural_check",
        crate::state::Stage::Review => "review",
        crate::state::Stage::StateRecord => "state_record",
    }
}
