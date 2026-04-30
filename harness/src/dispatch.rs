use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::adapter::ScopeEnforcementMode;
use crate::queue::{Slice, SliceStatus};
use crate::state::{Stage, load_active_slice};
use crate::validation::load_queue_file;

#[derive(Debug, Clone)]
pub struct PrepareDispatchOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub active_state_path: PathBuf,
    pub author_output_dir: PathBuf,
    pub dispatch_root: PathBuf,
    pub qa_report_path: Option<PathBuf>,
    pub latest_qa_report_path: Option<PathBuf>,
    pub slice_id: String,
    pub dispatch_kind: Option<AuthorDispatchKind>,
}

#[derive(Debug, Clone, Copy, Serialize, ValueEnum, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorDispatchKind {
    Initial,
    Retry,
    MicroCorrection,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrepareDispatchResult {
    pub slice_id: String,
    pub stage: Stage,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch_kind: Option<AuthorDispatchKind>,
    pub prompt_path: String,
    pub stdout_capture_path: String,
    pub evidence_bundle_path: String,
    pub scope_enforcement: ScopeEnforcementMode,
    pub allowed_write_globs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qa_report_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_qa_report_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_output_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_written_artifacts: Vec<String>,
}

pub fn prepare_dispatch(options: PrepareDispatchOptions) -> Result<PrepareDispatchResult> {
    if options.slice_id.trim().is_empty() {
        bail!("missing `slice_id`");
    }

    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let queue_path = resolve_workspace_path(&workspace_root, &options.queue_path);
    let active_state_path = resolve_workspace_path(&workspace_root, &options.active_state_path);
    let author_output_dir = resolve_workspace_path(&workspace_root, &options.author_output_dir);
    let dispatch_root = resolve_workspace_path(&workspace_root, &options.dispatch_root);
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

    let queue = load_queue_file(&queue_path)?;
    let active_state = load_active_slice(&active_state_path)?;

    if active_state.slice_id != options.slice_id {
        bail!(
            "active slice mismatch: expected `{}`, found `{}`",
            options.slice_id,
            active_state.slice_id
        );
    }

    let slice = queue
        .slices
        .iter()
        .find(|slice| slice.id == options.slice_id)
        .with_context(|| format!("slice `{}` not found", options.slice_id))?;

    if slice.status != SliceStatus::InProgress {
        bail!(
            "cannot prepare dispatch for slice `{}` from status `{}`",
            slice.id,
            slice_status_name(slice.status)
        );
    }

    let scope_enforcement = if active_state
        .degraded_capabilities
        .iter()
        .any(|feature| feature == "pre_write_scope_enforcement")
    {
        ScopeEnforcementMode::Advisory
    } else {
        ScopeEnforcementMode::Hard
    };

    let paths = DispatchPaths {
        workspace_root: &workspace_root,
        author_output_dir: &author_output_dir,
        dispatch_root: &dispatch_root,
        qa_report_path: &qa_report_path,
        latest_qa_report_path: &latest_qa_report_path,
    };

    match active_state.stage {
        Stage::Author => prepare_author_dispatch(
            &paths,
            slice,
            &active_state,
            scope_enforcement,
            options.dispatch_kind,
        ),
        Stage::Review => prepare_review_dispatch(&paths, slice, &active_state, scope_enforcement),
        Stage::StructuralCheck | Stage::StateRecord => bail!(
            "dispatch preparation is only supported for `author` and `review`, not `{}`",
            stage_name(active_state.stage)
        ),
    }
}

/// Bundle of paths shared by author and review dispatch preparation. Reduces
/// argument lists below the clippy `too_many_arguments` threshold without
/// flattening the workspace-vs-stage-specific concerns.
struct DispatchPaths<'a> {
    workspace_root: &'a Path,
    author_output_dir: &'a Path,
    dispatch_root: &'a Path,
    qa_report_path: &'a Path,
    latest_qa_report_path: &'a Path,
}

fn prepare_author_dispatch(
    paths: &DispatchPaths<'_>,
    slice: &Slice,
    active_state: &crate::state::ActiveSliceState,
    scope_enforcement: ScopeEnforcementMode,
    requested_dispatch_kind: Option<AuthorDispatchKind>,
) -> Result<PrepareDispatchResult> {
    let DispatchPaths {
        workspace_root,
        author_output_dir,
        dispatch_root,
        qa_report_path,
        ..
    } = *paths;
    let qa_report_exists = qa_report_path.is_file();
    let dispatch_kind = match requested_dispatch_kind {
        Some(AuthorDispatchKind::Initial) => AuthorDispatchKind::Initial,
        Some(AuthorDispatchKind::Retry) => {
            if !qa_report_exists {
                bail!(
                    "author retry dispatch requested for slice `{}` but QA report is missing at {}",
                    slice.id,
                    display_path(qa_report_path)
                );
            }
            AuthorDispatchKind::Retry
        }
        Some(AuthorDispatchKind::MicroCorrection) => {
            if !qa_report_exists {
                bail!(
                    "micro-correction dispatch requested for slice `{}` but QA report is missing at {}",
                    slice.id,
                    display_path(qa_report_path)
                );
            }
            AuthorDispatchKind::MicroCorrection
        }
        None if qa_report_exists => AuthorDispatchKind::Retry,
        None => AuthorDispatchKind::Initial,
    };

    let prompt_path = dispatch_root
        .join(&slice.id)
        .join(prompt_file_name(Stage::Author, Some(dispatch_kind)));
    let stdout_capture_path = author_output_dir.join(format!("{}.md", safe_file_name(&slice.id)));

    let prompt = render_author_prompt(
        workspace_root,
        slice,
        active_state,
        scope_enforcement,
        dispatch_kind,
        qa_report_exists.then_some(qa_report_path),
    );
    write_text_file(&prompt_path, &prompt)?;

    Ok(PrepareDispatchResult {
        slice_id: slice.id.clone(),
        stage: Stage::Author,
        agent: active_state.active_agent.clone(),
        dispatch_kind: Some(dispatch_kind),
        prompt_path: display_path(&prompt_path),
        stdout_capture_path: display_path(&stdout_capture_path),
        evidence_bundle_path: display_workspace_path(
            workspace_root,
            &active_state.evidence_bundle_path,
        ),
        scope_enforcement,
        allowed_write_globs: active_state.allowed_write_globs.clone(),
        qa_report_path: qa_report_exists.then(|| display_path(qa_report_path)),
        latest_qa_report_path: None,
        author_output_path: Some(display_path(&stdout_capture_path)),
        required_written_artifacts: Vec::new(),
    })
}

fn prepare_review_dispatch(
    paths: &DispatchPaths<'_>,
    slice: &Slice,
    active_state: &crate::state::ActiveSliceState,
    scope_enforcement: ScopeEnforcementMode,
) -> Result<PrepareDispatchResult> {
    let DispatchPaths {
        workspace_root,
        author_output_dir,
        dispatch_root,
        qa_report_path,
        latest_qa_report_path,
    } = *paths;
    let author_output_path = author_output_dir.join(format!("{}.md", safe_file_name(&slice.id)));
    if !author_output_path.is_file() {
        bail!(
            "author output for slice `{}` is missing at {}",
            slice.id,
            display_path(&author_output_path)
        );
    }

    let prompt_path = dispatch_root
        .join(&slice.id)
        .join(prompt_file_name(Stage::Review, None));
    let stdout_capture_path = workspace_root
        .join(".mutagen/state/review-output")
        .join(format!("{}.md", safe_file_name(&slice.id)));

    let prompt = render_review_prompt(
        workspace_root,
        slice,
        active_state,
        scope_enforcement,
        &author_output_path,
        qa_report_path,
        latest_qa_report_path,
    );
    write_text_file(&prompt_path, &prompt)?;

    Ok(PrepareDispatchResult {
        slice_id: slice.id.clone(),
        stage: Stage::Review,
        agent: active_state.active_agent.clone(),
        dispatch_kind: None,
        prompt_path: display_path(&prompt_path),
        stdout_capture_path: display_path(&stdout_capture_path),
        evidence_bundle_path: display_workspace_path(
            workspace_root,
            &active_state.evidence_bundle_path,
        ),
        scope_enforcement,
        allowed_write_globs: active_state.allowed_write_globs.clone(),
        qa_report_path: Some(display_path(qa_report_path)),
        latest_qa_report_path: Some(display_path(latest_qa_report_path)),
        author_output_path: Some(display_path(&author_output_path)),
        required_written_artifacts: vec![
            display_path(qa_report_path),
            display_path(latest_qa_report_path),
        ],
    })
}

fn render_author_prompt(
    workspace_root: &Path,
    slice: &Slice,
    active_state: &crate::state::ActiveSliceState,
    scope_enforcement: ScopeEnforcementMode,
    dispatch_kind: AuthorDispatchKind,
    qa_report_path: Option<&Path>,
) -> String {
    let mut body = Vec::new();

    body.push("# Author Dispatch".to_string());
    body.push(String::new());
    body.push(format!("- Slice: {}", slice.id));
    body.push(format!(
        "- Title: {}",
        fallback_text(&slice.title, "Untitled slice")
    ));
    body.push(format!("- Stage: {}", stage_name(Stage::Author)));
    body.push(format!(
        "- Dispatch kind: {}",
        author_dispatch_kind_name(dispatch_kind)
    ));
    body.push(format!("- Agent: {}", active_state.active_agent));
    body.push(format!("- Layer: L{}", slice.layer));
    body.push(format!(
        "- Bounded context: {}",
        fallback_text(&slice.bounded_context, "unspecified")
    ));
    body.push(format!(
        "- Objective: {}",
        fallback_text(&slice.objective, "unspecified")
    ));
    body.push(format!("- Target LOC: {}", slice.target_loc));
    body.push(format!(
        "- Context file to update: {}",
        fallback_text(&slice.context_to_update, "unspecified")
    ));
    body.push(format!(
        "- Review required: {}",
        if slice.review_required { "yes" } else { "no" }
    ));
    body.push(format!(
        "- Scope enforcement: {}",
        scope_enforcement_name(scope_enforcement)
    ));
    body.push(String::new());
    body.push("## Evidence".to_string());
    body.push(format!(
        "- Read this bundle once before coding: {}",
        display_workspace_path(workspace_root, &active_state.evidence_bundle_path)
    ));
    body.push("- Do not re-read PRD / ADR / DDD / ISC / DSD docs unless the harness tells you the evidence bundle is stale.".to_string());
    body.push(String::new());
    body.push("## Slice contract".to_string());
    push_traces(&mut body, &slice.traces_to);
    push_list(
        &mut body,
        "Implementation details",
        &slice.implementation_details,
        "None recorded.",
    );
    body.push("### Verification expectations".to_string());
    body.push(format!(
        "- Acceptance: {}",
        fallback_text(&slice.verification_steps.acceptance, "unspecified")
    ));
    body.push(format!(
        "- ISC detection: {}",
        fallback_text(&slice.verification_steps.isc_detection, "unspecified")
    ));
    body.push(format!(
        "- DSD conformance: {}",
        fallback_text(&slice.verification_steps.dsd_conformance, "unspecified")
    ));
    if slice.human_check_needed.required {
        body.push(format!(
            "- Human check required: {}",
            fallback_text(&slice.human_check_needed.reason, "reason missing")
        ));
    }
    body.push(String::new());
    body.push("## Write scope".to_string());
    push_list(
        &mut body,
        "Allowed write globs",
        &active_state.allowed_write_globs,
        "No write globs recorded.",
    );
    body.push(String::new());
    body.push("## Required instructions".to_string());
    body.push("- Stay inside the allowed write globs. If you need a path outside them, stop and surface the gap instead of widening scope silently.".to_string());
    body.push(format!(
        "- Emit the required State Update block for {}.",
        fallback_text(&slice.context_to_update, "the configured context file")
    ));
    body.push("- Do not edit the context file directly. The harness applies the State Update block during state_record.".to_string());
    body.push("- Follow the persona's output contract exactly. Keep the execution summary terse and artifact-oriented.".to_string());

    if let Some(path) = qa_report_path {
        body.push(format!(
            "- Prior Tiger Claw QA report: {}.",
            display_workspace_file_path(workspace_root, path)
        ));
        body.push("- Address every Suggested Fix in that report. Keep the change minimal and do not wander into adjacent cleanup.".to_string());
    }

    if dispatch_kind == AuthorDispatchKind::MicroCorrection {
        body.push("- This is a bounded mechanical correction, not a full rewrite. Prefer the smallest change that clears the defect.".to_string());
    }

    body.join("\n")
}

fn render_review_prompt(
    workspace_root: &Path,
    slice: &Slice,
    active_state: &crate::state::ActiveSliceState,
    scope_enforcement: ScopeEnforcementMode,
    author_output_path: &Path,
    qa_report_path: &Path,
    latest_qa_report_path: &Path,
) -> String {
    let mut body = Vec::new();

    body.push("# Review Dispatch".to_string());
    body.push(String::new());
    body.push(format!("- Slice: {}", slice.id));
    body.push(format!(
        "- Title: {}",
        fallback_text(&slice.title, "Untitled slice")
    ));
    body.push(format!("- Stage: {}", stage_name(Stage::Review)));
    body.push(format!("- Agent: {}", active_state.active_agent));
    body.push(format!("- Layer: L{}", slice.layer));
    body.push(format!(
        "- Bounded context: {}",
        fallback_text(&slice.bounded_context, "unspecified")
    ));
    body.push(format!(
        "- Objective: {}",
        fallback_text(&slice.objective, "unspecified")
    ));
    body.push(format!(
        "- Scope enforcement: {}",
        scope_enforcement_name(scope_enforcement)
    ));
    body.push(String::new());
    body.push("## Evidence".to_string());
    body.push(format!(
        "- Read this bundle once before hunting defects: {}",
        display_workspace_path(workspace_root, &active_state.evidence_bundle_path)
    ));
    body.push(format!(
        "- Author output to review: {}",
        display_workspace_file_path(workspace_root, author_output_path)
    ));
    body.push("- Do not re-read PRD / ADR / DDD / ISC / DSD docs unless the harness tells you the evidence bundle is stale.".to_string());
    body.push(String::new());
    body.push("## Slice contract".to_string());
    push_traces(&mut body, &slice.traces_to);
    push_list(
        &mut body,
        "Implementation details",
        &slice.implementation_details,
        "None recorded.",
    );
    body.push("### Verification expectations".to_string());
    body.push(format!(
        "- Acceptance: {}",
        fallback_text(&slice.verification_steps.acceptance, "unspecified")
    ));
    body.push(format!(
        "- ISC detection: {}",
        fallback_text(&slice.verification_steps.isc_detection, "unspecified")
    ));
    body.push(format!(
        "- DSD conformance: {}",
        fallback_text(&slice.verification_steps.dsd_conformance, "unspecified")
    ));
    body.push(String::new());
    body.push("## Write scope".to_string());
    push_list(
        &mut body,
        "Allowed write globs",
        &active_state.allowed_write_globs,
        "No write globs recorded.",
    );
    body.push(String::new());
    body.push("## Required instructions".to_string());
    body.push("- Write adversarial tests only inside the QA scope. Do not modify production code or the author's own tests.".to_string());
    body.push(format!(
        "- Write the QA report to: {}",
        display_workspace_file_path(workspace_root, qa_report_path)
    ));
    body.push(format!(
        "- Write the convenience copy to: {}",
        display_workspace_file_path(workspace_root, latest_qa_report_path)
    ));
    body.push("- Follow the persona's output contract exactly. The machine-readable Retry Contract block is mandatory.".to_string());

    body.join("\n")
}

fn push_traces(lines: &mut Vec<String>, traces: &crate::queue::TraceSet) {
    lines.push("### Traces-to".to_string());
    push_list(lines, "PRD", &traces.prd, "None cited.");
    push_list(lines, "ADR", &traces.adr, "None cited.");
    push_list(lines, "DDD", &traces.ddd, "None cited.");
    push_list(lines, "ISC", &traces.isc, "None cited.");
    push_list(lines, "DSD", &traces.dsd, "None cited.");
}

fn push_list(lines: &mut Vec<String>, heading: &str, entries: &[String], empty_message: &str) {
    lines.push(format!("### {}", heading));
    if entries.is_empty() {
        lines.push(format!("- {}", empty_message));
        return;
    }

    for entry in entries {
        lines.push(format!("- {}", fallback_text(entry, empty_message)));
    }
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

    fs::write(path, format!("{body}\n"))
        .with_context(|| format!("failed to write {}", display_path(path)))
}

fn resolve_workspace_root(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        path.canonicalize()
            .with_context(|| format!("failed to resolve workspace root {}", display_path(path)))
    } else {
        bail!("workspace root does not exist: {}", display_path(path));
    }
}

fn resolve_workspace_path(workspace_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

fn display_workspace_path(workspace_root: &Path, value: &str) -> String {
    let candidate = Path::new(value);
    let resolved = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace_root.join(candidate)
    };
    let normalized_root = normalize_path_separators(workspace_root);
    let normalized_path = normalize_path_separators(&resolved);
    let normalized_root = normalized_root.trim_end_matches('/');

    normalized_path
        .strip_prefix(&format!("{normalized_root}/"))
        .map(str::to_string)
        .unwrap_or(normalized_path)
}

fn display_workspace_file_path(workspace_root: &Path, path: &Path) -> String {
    let normalized_root = normalize_path_separators(workspace_root);
    let normalized_path = normalize_path_separators(path);
    let normalized_root = normalized_root.trim_end_matches('/');

    normalized_path
        .strip_prefix(&format!("{normalized_root}/"))
        .map(str::to_string)
        .unwrap_or(normalized_path)
}

fn prompt_file_name(stage: Stage, dispatch_kind: Option<AuthorDispatchKind>) -> String {
    match (stage, dispatch_kind) {
        (Stage::Author, Some(kind)) => {
            format!("author-{}.md", author_dispatch_kind_slug(kind))
        }
        (Stage::Review, _) => "review.md".to_string(),
        _ => "dispatch.md".to_string(),
    }
}

fn author_dispatch_kind_name(kind: AuthorDispatchKind) -> &'static str {
    match kind {
        AuthorDispatchKind::Initial => "initial",
        AuthorDispatchKind::Retry => "retry",
        AuthorDispatchKind::MicroCorrection => "micro_correction",
    }
}

fn author_dispatch_kind_slug(kind: AuthorDispatchKind) -> &'static str {
    match kind {
        AuthorDispatchKind::Initial => "initial",
        AuthorDispatchKind::Retry => "retry",
        AuthorDispatchKind::MicroCorrection => "micro-correction",
    }
}

fn scope_enforcement_name(mode: ScopeEnforcementMode) -> &'static str {
    match mode {
        ScopeEnforcementMode::Hard => "hard",
        ScopeEnforcementMode::Advisory => "advisory",
    }
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

fn fallback_text(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
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
