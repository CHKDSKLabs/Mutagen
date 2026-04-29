use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::adapter::HostKind;
use crate::notifications::{NotificationEvent, layer_complete_notification};
use crate::queue::{
    BishopVerdict, KaraiStructuralVerdict, Slice, SliceStatus, SliceVerdicts, TigerClawVerdict,
};
use crate::state::{Stage, load_active_slice};
use crate::state_update::{
    ParsedStateUpdate, apply_state_update_block, context_contains_state_update, parse_state_update,
};
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct FinalizeSliceOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub active_state_path: PathBuf,
    pub dispatch_log_path: PathBuf,
    pub summary_root: PathBuf,
    pub slice_id: String,
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FinalizeSliceResult {
    pub queue_path: String,
    pub active_state_path: String,
    pub dispatch_log_path: String,
    pub summary_path: String,
    pub slice_id: String,
    pub status: SliceStatus,
    pub completed_at: String,
    pub attempts: u32,
    pub micro_correction: bool,
    pub retry_path: String,
    pub state_verified: bool,
    pub files_touched: Vec<String>,
    pub verdicts: SliceVerdicts,
    pub duration: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u64>,
    pub layer: u32,
    pub layer_complete: bool,
    pub completed_in_layer: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_pending_slice_id: Option<String>,
    pub completion_marker: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notifications: Vec<NotificationEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
enum DispatchStatus {
    Completed,
    QaGap,
    QaSkip,
}

#[derive(Debug, Clone, Serialize)]
struct DispatchLogEntry {
    slice_id: String,
    title: String,
    agent: String,
    host: HostKind,
    layer: u32,
    bounded_context: String,
    status: DispatchStatus,
    completed_at: String,
    attempts: u32,
    micro_correction: bool,
    state_verified: bool,
    summary_path: String,
    evidence_bundle_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    qa_report_path: Option<String>,
    verdicts: SliceVerdicts,
    notes: String,
}

pub fn finalize_slice(options: FinalizeSliceOptions) -> Result<FinalizeSliceResult> {
    if options.slice_id.trim().is_empty() {
        bail!("missing `slice_id`");
    }

    if options.completed_at.trim().is_empty() {
        bail!("missing `completed_at`");
    }

    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let queue_path = resolve_workspace_path(&workspace_root, &options.queue_path);
    let active_state_path = resolve_workspace_path(&workspace_root, &options.active_state_path);
    let dispatch_log_path = resolve_workspace_path(&workspace_root, &options.dispatch_log_path);
    let summary_root = resolve_workspace_path(&workspace_root, &options.summary_root);

    let mut queue = load_queue_file(&queue_path)?;
    let active_state = load_active_slice(&active_state_path)?;

    if active_state.slice_id != options.slice_id {
        bail!(
            "active slice mismatch: expected `{}`, found `{}`",
            options.slice_id,
            active_state.slice_id
        );
    }

    if active_state.stage != Stage::StateRecord {
        bail!(
            "cannot finalize slice `{}` while active stage is `{}`",
            options.slice_id,
            stage_name(active_state.stage)
        );
    }

    let evidence_bundle_path = resolve_workspace_path(
        &workspace_root,
        Path::new(&active_state.evidence_bundle_path),
    );
    if !evidence_bundle_path.exists() {
        bail!(
            "evidence bundle missing at {}",
            display_path(&evidence_bundle_path)
        );
    }

    let author_output_path = workspace_root
        .join(".mutagen/state/author-output")
        .join(format!("{}.md", options.slice_id));
    let author_output = fs::read_to_string(&author_output_path).with_context(|| {
        format!(
            "failed to read author output at {}",
            display_path(&author_output_path)
        )
    })?;
    let state_update = parse_state_update(&author_output, &options.slice_id)?;
    let state_verified =
        apply_and_verify_state_update(&workspace_root, &active_state, &state_update)?;
    let files_touched = extract_artifact_paths(&author_output);

    let now_unix_ms = now_unix_ms()?;
    let duration_seconds = active_state
        .started_at_unix_ms
        .map(|started_at| now_unix_ms.saturating_sub(started_at) / 1_000);
    let duration = duration_seconds
        .map(format_duration)
        .unwrap_or_else(|| "unknown".to_string());

    let (slice_snapshot, tiger_claw_verdict, micro_correction) = {
        let slice = queue
            .slice_mut(&options.slice_id)
            .with_context(|| format!("slice `{}` not found", options.slice_id))?;

        if slice.status != SliceStatus::InProgress {
            bail!(
                "cannot finalize slice `{}` from status `{}`",
                slice.id,
                slice_status_name(slice.status)
            );
        }

        if slice.verdicts.karai_structural != Some(KaraiStructuralVerdict::Pass) {
            bail!(
                "slice `{}` cannot finalize without `karai_structural: pass`",
                slice.id
            );
        }

        if slice.verdicts.bishop.is_none() {
            slice.verdicts.bishop = Some(BishopVerdict::Skip);
        }

        let tiger_claw_verdict = slice
            .verdicts
            .tiger_claw
            .with_context(|| format!("slice `{}` is missing a Tiger Claw verdict", slice.id))?;

        if tiger_claw_verdict == TigerClawVerdict::Defect {
            bail!(
                "slice `{}` cannot finalize with `tiger_claw: defect`",
                slice.id
            );
        }

        slice.status = SliceStatus::Completed;
        slice.completed_at = Some(options.completed_at.clone());
        slice.escalation_reason = None;

        (
            slice.clone(),
            tiger_claw_verdict,
            slice.verdicts.micro_correction.unwrap_or(false),
        )
    };

    let retry_path = derive_retry_path(slice_snapshot.attempts, micro_correction);
    let summary_path = summary_root.join(&slice_snapshot.id).join("summary.md");
    let qa_report_path = workspace_root
        .join("reviews")
        .join(&slice_snapshot.id)
        .join("tiger-claw.md");
    let qa_report_path_display = qa_report_path
        .exists()
        .then(|| display_relative_to_workspace(&workspace_root, &qa_report_path));
    let evidence_bundle_path_display =
        display_relative_to_workspace(&workspace_root, &evidence_bundle_path);
    let summary_body = render_summary(
        &slice_snapshot,
        &options.completed_at,
        &duration,
        micro_correction,
        &files_touched,
        &retry_path,
        qa_report_path_display.as_deref(),
        &evidence_bundle_path_display,
    );

    let dispatch_entry = DispatchLogEntry {
        slice_id: slice_snapshot.id.clone(),
        title: slice_snapshot.title.clone(),
        agent: slice_snapshot.author_agent.clone(),
        host: active_state.host,
        layer: slice_snapshot.layer,
        bounded_context: slice_snapshot.bounded_context.clone(),
        status: dispatch_status_for(tiger_claw_verdict),
        completed_at: options.completed_at.clone(),
        attempts: slice_snapshot.attempts,
        micro_correction,
        state_verified,
        summary_path: display_relative_to_workspace(&workspace_root, &summary_path),
        evidence_bundle_path: evidence_bundle_path_display,
        qa_report_path: qa_report_path_display,
        verdicts: slice_snapshot.verdicts.clone(),
        notes: format!(
            "Tiger Claw: {}; attempts={}; micro_correction={}",
            tiger_claw_name(tiger_claw_verdict),
            slice_snapshot.attempts,
            micro_correction
        ),
    };

    write_json_file(&queue_path, &queue)?;
    write_text_file(&summary_path, &summary_body)?;
    append_dispatch_log(&dispatch_log_path, &dispatch_entry)?;
    fs::remove_file(&active_state_path).with_context(|| {
        format!(
            "failed to clear active slice at {}",
            display_path(&active_state_path)
        )
    })?;

    let layer_complete = !queue.slices.iter().any(|candidate| {
        candidate.layer == slice_snapshot.layer && candidate.status.is_ready_candidate()
    });
    let completed_in_layer = queue
        .slices
        .iter()
        .filter(|candidate| {
            candidate.layer == slice_snapshot.layer && candidate.status == SliceStatus::Completed
        })
        .count();
    let next_pending_slice_id = queue
        .slices
        .iter()
        .find(|candidate| candidate.status.is_ready_candidate())
        .map(|candidate| candidate.id.clone());
    let completion_marker = render_completion_marker(
        &slice_snapshot.id,
        tiger_claw_verdict,
        slice_snapshot.attempts,
        micro_correction,
    );
    let notifications = if layer_complete {
        vec![layer_complete_notification(
            slice_snapshot.layer,
            completed_in_layer,
            next_pending_slice_id.as_deref(),
        )]
    } else {
        Vec::new()
    };

    Ok(FinalizeSliceResult {
        queue_path: display_path(&queue_path),
        active_state_path: display_path(&active_state_path),
        dispatch_log_path: display_path(&dispatch_log_path),
        summary_path: display_path(&summary_path),
        slice_id: slice_snapshot.id,
        status: SliceStatus::Completed,
        completed_at: options.completed_at,
        attempts: slice_snapshot.attempts,
        micro_correction,
        retry_path,
        state_verified,
        files_touched,
        verdicts: slice_snapshot.verdicts,
        duration,
        duration_seconds,
        layer: slice_snapshot.layer,
        layer_complete,
        completed_in_layer,
        next_pending_slice_id,
        completion_marker,
        notifications,
    })
}

fn apply_and_verify_state_update(
    workspace_root: &Path,
    active_state: &crate::state::ActiveSliceState,
    state_update: &ParsedStateUpdate,
) -> Result<bool> {
    let context_path =
        resolve_workspace_path(workspace_root, Path::new(&active_state.context_to_update));

    apply_state_update_block(&context_path, state_update)?;
    if !context_contains_state_update(&context_path, &state_update.marker)? {
        bail!(
            "state update marker `{}` not found in {}",
            state_update.marker,
            display_path(&context_path)
        );
    }

    Ok(true)
}

fn render_summary(
    slice: &Slice,
    completed_at: &str,
    duration: &str,
    micro_correction: bool,
    files_touched: &[String],
    retry_path: &str,
    qa_report_path: Option<&str>,
    evidence_bundle_path: &str,
) -> String {
    let mut body = String::new();
    body.push_str(&format!("# Slice summary — {}\n", slice.id));
    body.push_str(&format!("**Title:** {}\n", slice.title));
    body.push_str(&format!("**Author:** {}\n", slice.author_agent));
    body.push_str(&format!(
        "**Layer / Context:** L{} / {}\n",
        slice.layer, slice.bounded_context
    ));
    body.push_str(&format!("**Completed at:** {}\n", completed_at));
    body.push_str(&format!("**Duration:** {}\n", duration));
    body.push_str(&format!(
        "**Attempts:** {} (micro_correction: {})\n\n",
        slice.attempts, micro_correction
    ));

    body.push_str("## Verdicts\n");
    body.push_str(&format!(
        "- Karai structural: {}\n",
        structural_verdict_name(slice.verdicts.karai_structural)
    ));
    body.push_str("- Bishop: skip (disabled)\n");
    body.push_str(&format!(
        "- Tiger Claw: {}\n\n",
        tiger_claw_name(slice.verdicts.tiger_claw.unwrap_or(TigerClawVerdict::Skip))
    ));

    body.push_str("## Files touched\n");
    if files_touched.is_empty() {
        body.push_str("- none recorded\n\n");
    } else {
        for path in files_touched {
            body.push_str(&format!("- `{}`\n", path));
        }
        body.push('\n');
    }

    body.push_str("## Advisories logged\n");
    body.push_str("none\n\n");

    body.push_str("## Retry path\n");
    body.push_str(retry_path);
    body.push_str("\n\n");

    body.push_str("## Reports\n");
    match qa_report_path {
        Some(path) => body.push_str(&format!("- QA: `{}`\n", path)),
        None => body.push_str("- QA: none\n"),
    }
    body.push_str(&format!("- Evidence: `{}`\n", evidence_bundle_path));

    body
}

fn append_dispatch_log(path: &Path, entry: &DispatchLogEntry) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(path)
            )
        })?;
    }

    let line = serde_json::to_string(entry).context("failed to serialize dispatch log entry")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", display_path(path)))?;
    writeln!(file, "{line}")
        .with_context(|| format!("failed to append dispatch log at {}", display_path(path)))
}

fn extract_artifact_paths(author_output: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut in_artifact_section = false;

    for line in author_output.lines() {
        let trimmed = line.trim();

        if let Some(heading) = heading_name(trimmed) {
            if is_artifact_heading(heading) {
                in_artifact_section = true;
                continue;
            }

            if in_artifact_section {
                break;
            }
        }

        if !in_artifact_section {
            continue;
        }

        collect_paths_from_line(trimmed, &mut paths);
    }

    if paths.is_empty() {
        for line in author_output.lines() {
            collect_paths_from_line(line.trim(), &mut paths);
        }
    }

    paths
}

fn heading_name(line: &str) -> Option<&str> {
    for prefix in ["#### ", "### ", "## "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(rest.trim());
        }
    }

    None
}

fn is_artifact_heading(heading: &str) -> bool {
    matches!(
        heading,
        "Code Artifacts" | "Drafted Artefacts" | "Infrastructure Artifacts"
    )
}

fn collect_paths_from_line(line: &str, paths: &mut Vec<String>) {
    let mut in_backticks = false;
    let mut token = String::new();

    for ch in line.chars() {
        if ch == '`' {
            if in_backticks && looks_like_path(&token) && !paths.contains(&token) {
                paths.push(token.clone());
            }
            in_backticks = !in_backticks;
            token.clear();
            continue;
        }

        if in_backticks {
            token.push(ch);
        }
    }

    let Some(rest) = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("1. "))
    else {
        return;
    };

    let candidate = rest
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_matches(|ch: char| matches!(ch, '`' | '(' | ')' | '[' | ']' | ',' | ':'));

    if looks_like_path(candidate) && !paths.contains(&candidate.to_string()) {
        paths.push(candidate.to_string());
    }
}

fn looks_like_path(value: &str) -> bool {
    if value.is_empty() || value.starts_with("http://") || value.starts_with("https://") {
        return false;
    }

    value.contains('/')
        || value.ends_with(".md")
        || value.ends_with(".rs")
        || value.ends_with(".ts")
        || value.ends_with(".tsx")
        || value.ends_with(".js")
        || value.ends_with(".jsx")
        || value.ends_with(".py")
        || value.ends_with(".sql")
        || value.ends_with(".toml")
        || value.ends_with(".yml")
        || value.ends_with(".yaml")
        || value.ends_with(".json")
        || value.ends_with(".sh")
}

fn derive_retry_path(attempts: u32, micro_correction: bool) -> String {
    if micro_correction {
        return format!("micro-correction on attempt {}", attempts.max(1));
    }

    if attempts <= 1 {
        return "first-pass clean".to_string();
    }

    let retries = attempts - 1;
    if retries == 1 {
        "1 Tiger Claw retry cleared".to_string()
    } else {
        format!("{retries} Tiger Claw retries cleared")
    }
}

fn render_completion_marker(
    slice_id: &str,
    tiger_claw_verdict: TigerClawVerdict,
    attempts: u32,
    micro_correction: bool,
) -> String {
    let mut marker = format!(
        "✔ {} — {}, attempts={}",
        slice_id,
        tiger_claw_name(tiger_claw_verdict),
        attempts
    );

    if micro_correction {
        marker.push_str(", micro_correction");
    }

    marker
}

fn dispatch_status_for(verdict: TigerClawVerdict) -> DispatchStatus {
    match verdict {
        TigerClawVerdict::Clean => DispatchStatus::Completed,
        TigerClawVerdict::Gap => DispatchStatus::QaGap,
        TigerClawVerdict::Skip => DispatchStatus::QaSkip,
        TigerClawVerdict::Defect => DispatchStatus::Completed,
    }
}

fn structural_verdict_name(verdict: Option<KaraiStructuralVerdict>) -> &'static str {
    match verdict {
        Some(KaraiStructuralVerdict::Pass) => "pass",
        Some(KaraiStructuralVerdict::Fail) => "fail",
        None => "unknown",
    }
}

fn tiger_claw_name(verdict: TigerClawVerdict) -> &'static str {
    match verdict {
        TigerClawVerdict::Clean => "clean",
        TigerClawVerdict::Gap => "gap",
        TigerClawVerdict::Defect => "defect",
        TigerClawVerdict::Skip => "skip",
    }
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let remaining_seconds = seconds % 60;

    if hours > 0 {
        return format!("{hours}h {minutes}m {remaining_seconds}s");
    }

    if minutes > 0 {
        return format!("{minutes}m {remaining_seconds}s");
    }

    format!("{remaining_seconds}s")
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

fn write_text_file(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(path)
            )
        })?;
    }

    fs::write(path, body).with_context(|| format!("failed to write {}", display_path(path)))
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

fn display_relative_to_workspace(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .map(normalize_path_separators)
        .ok()
        .or_else(|| strip_normalized_workspace_prefix(workspace_root, path))
        .unwrap_or_else(|| normalize_path_separators(path))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn normalize_path_separators(path: &Path) -> String {
    let normalized = display_path(path).replace('\\', "/");
    normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .to_string()
}

fn strip_normalized_workspace_prefix(workspace_root: &Path, path: &Path) -> Option<String> {
    let normalized_root = normalize_path_separators(workspace_root);
    let normalized_path = normalize_path_separators(path);
    let normalized_root = normalized_root.trim_end_matches('/');

    normalized_path
        .strip_prefix(&format!("{normalized_root}/"))
        .map(str::to_string)
}

fn now_unix_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before unix epoch")?
        .as_millis() as u64)
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
