use crate::adapter::HostKind;
use crate::finalize::{FinalizeSliceOptions, finalize_slice};
use crate::project::{
    ProjectCommandKind, ProjectDashboardOptions, ProjectDoctorOptions,
    ProjectExecuteFeatureOptions, ProjectFeatureFlowOptions, ProjectFeatureProgressOptions,
    ProjectInspectOptions, ProjectPreviewCheckOptions, ProjectPreviewLifecycleOptions,
    ProjectPreviewPlanOptions, ProjectRepairOptions, ProjectRunCommandOptions,
    ProjectVerifyGeneratedOptions, dashboard_project, doctor_project, execute_feature,
    feature_flow, feature_progress, inspect_project, preview_check, preview_plan, preview_start,
    preview_status, preview_stop, repair_project, run_project_command, verify_generated_project,
};
use crate::queue::SliceStatus;
use crate::queue_update::{UpdateSliceOptions, update_slice};
use crate::runtime::{PrepareNextOptions, prepare_next};
use crate::validation::{load_queue_file, validate_queue_file};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tiny_http::{Header, Method, Response, Server, StatusCode};

#[derive(Debug, Clone)]
pub struct DashboardServeOptions {
    pub workspace_root: PathBuf,
    pub bind: String,
    pub port: u16,
    pub host: HostKind,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DashboardServeResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub bind: String,
    pub port: u16,
    pub url: String,
}

#[derive(Debug, Deserialize)]
struct FeatureFlowRequest {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
struct ExecuteFeatureRequest {
    feature_id: String,
    #[serde(default)]
    dry_run: bool,
    #[serde(default)]
    host: Option<HostKind>,
}

#[derive(Debug, Deserialize)]
struct RunCommandRequest {
    kind: ProjectCommandKind,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
struct SliceOperatorRequest {
    slice_id: String,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Deserialize)]
struct QueuePrepareRequest {
    #[serde(default)]
    host: Option<HostKind>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
struct RepairScaffoldRequest {
    #[serde(default)]
    force: bool,
}

pub fn dashboard_server_info(options: &DashboardServeOptions) -> Result<DashboardServeResult> {
    let workspace_root = options
        .workspace_root
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", options.workspace_root.display()))?;

    Ok(DashboardServeResult {
        ok: true,
        status: "listening".to_string(),
        workspace_root: workspace_root.to_string_lossy().into_owned(),
        bind: options.bind.clone(),
        port: options.port,
        url: format!("http://{}:{}/", options.bind, options.port),
    })
}

pub fn serve_dashboard(options: DashboardServeOptions) -> Result<()> {
    let info = dashboard_server_info(&options)?;
    let server = Server::http(format!("{}:{}", options.bind, options.port))
        .map_err(|error| anyhow::anyhow!("failed to bind {}: {error}", info.url))?;

    println!("{}", serde_json::to_string_pretty(&info)?);

    for mut request in server.incoming_requests() {
        let mut body = Vec::new();
        request
            .as_reader()
            .read_to_end(&mut body)
            .context("failed to read request body")?;
        let response = route_dashboard_request(
            request.method(),
            request.url(),
            &body,
            &options.workspace_root,
            options.host,
        );

        match response {
            Ok((status, content_type, body)) => {
                let mut http_response =
                    Response::from_string(body).with_status_code(StatusCode(status));
                if let Ok(header) =
                    Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
                {
                    http_response = http_response.with_header(header);
                }
                let _ = request.respond(http_response);
            }
            Err(error) => {
                let body = json!({
                    "ok": false,
                    "status": "error",
                    "message": error.to_string()
                })
                .to_string();
                let mut http_response =
                    Response::from_string(body).with_status_code(StatusCode(500));
                if let Ok(header) = Header::from_bytes(&b"Content-Type"[..], b"application/json") {
                    http_response = http_response.with_header(header);
                }
                let _ = request.respond(http_response);
            }
        }
    }

    Ok(())
}

fn route_dashboard_request(
    method: &Method,
    url: &str,
    body: &[u8],
    workspace_root: &Path,
    default_host: HostKind,
) -> Result<(u16, String, String)> {
    let (path, query) = split_url(url);

    match (method, path.as_str()) {
        (&Method::Get, "/") => Ok((
            200,
            "text/html; charset=utf-8".to_string(),
            dashboard_html(),
        )),
        (&Method::Get, "/healthz") => Ok((
            200,
            "application/json".to_string(),
            json!({
                "ok": true,
                "status": "ok"
            })
            .to_string(),
        )),
        (&Method::Get, "/api/dashboard") => {
            let body = dashboard_project(ProjectDashboardOptions {
                workspace_root: workspace_root.to_path_buf(),
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/doctor") => {
            let body = doctor_project(ProjectDoctorOptions {
                workspace_root: workspace_root.to_path_buf(),
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/preview-plan") => {
            let body = preview_plan(ProjectPreviewPlanOptions {
                workspace_root: workspace_root.to_path_buf(),
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/preview-status") => {
            let body = preview_status(ProjectPreviewLifecycleOptions {
                workspace_root: workspace_root.to_path_buf(),
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/preview-check") => {
            let body = preview_check(ProjectPreviewCheckOptions {
                workspace_root: workspace_root.to_path_buf(),
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/build-log") => {
            let limit = query_limit(&query).unwrap_or(20);
            let inspect = inspect_project(ProjectInspectOptions {
                workspace_root: workspace_root.to_path_buf(),
            })?;
            let log_path = workspace_root.join(&inspect.capsule.state.build_log);
            let entries = read_jsonl_tail(&log_path, limit)?;
            Ok((
                200,
                "application/json".to_string(),
                json!({
                    "ok": true,
                    "status": "ready",
                    "path": log_path.to_string_lossy(),
                    "entries": entries,
                })
                .to_string(),
            ))
        }
        (&Method::Get, "/api/activity-feed") => {
            let limit = query_limit(&query).unwrap_or(20);
            let body = activity_feed(workspace_root, limit)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/preview-log") => {
            let limit = query_limit(&query).unwrap_or(80);
            let preview = preview_status(ProjectPreviewLifecycleOptions {
                workspace_root: workspace_root.to_path_buf(),
            })?;
            let lines = read_text_tail(Path::new(&preview.log_path), limit)?;
            Ok((
                200,
                "application/json".to_string(),
                json!({
                    "ok": true,
                    "status": "ready",
                    "path": preview.log_path,
                    "lines": lines,
                })
                .to_string(),
            ))
        }
        (&Method::Get, "/api/feature-progress") => {
            let feature_id = query_value(&query, "feature_id")
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("`feature_id` query parameter is required"))?;
            let body = feature_progress(ProjectFeatureProgressOptions {
                workspace_root: workspace_root.to_path_buf(),
                feature_id,
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/slice-artifacts") => {
            let slice_id = query_value(&query, "slice_id")
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("`slice_id` query parameter is required"))?;
            let body = slice_artifacts(workspace_root, &slice_id)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/queue-status") => {
            let body = queue_status(workspace_root)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/slice-mark-blocked") => {
            let payload: SliceOperatorRequest = read_json_body(body)?;
            let body = operate_slice_status(
                workspace_root,
                &payload.slice_id,
                SliceStatus::BlockedRetry,
                Some(payload.reason),
            )?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/slice-resume") => {
            let payload: SliceOperatorRequest = read_json_body(body)?;
            let body = resume_slice(workspace_root, &payload.slice_id)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/slice-escalate") => {
            let payload: SliceOperatorRequest = read_json_body(body)?;
            let body = operate_slice_status(
                workspace_root,
                &payload.slice_id,
                SliceStatus::Escalated,
                Some(payload.reason),
            )?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/queue-prepare-next") => {
            let payload: QueuePrepareRequest = if body.is_empty() {
                QueuePrepareRequest {
                    host: None,
                    dry_run: false,
                }
            } else {
                read_json_body(body)?
            };
            let body = prepare_next(PrepareNextOptions {
                workspace_root: workspace_root.to_path_buf(),
                queue_path: workspace_root.join("slices/queue.json"),
                workflow_config_path: workspace_root.join(".claude/workflow.json"),
                active_state_path: workspace_root.join(".mutagen/state/active-slice.json"),
                host: payload.host.unwrap_or(default_host),
                dry_run: payload.dry_run,
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/slice-finalize") => {
            let payload: SliceOperatorRequest = read_json_body(body)?;
            let body = finalize_slice(FinalizeSliceOptions {
                workspace_root: workspace_root.to_path_buf(),
                queue_path: workspace_root.join("slices/queue.json"),
                active_state_path: workspace_root.join(".mutagen/state/active-slice.json"),
                dispatch_log_path: workspace_root.join(".mutagen/state/dispatch-log.jsonl"),
                summary_root: workspace_root.join("slices"),
                slice_id: payload.slice_id,
                completed_at: now_rfc3339()?,
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/preview-start") => {
            let body = preview_start(ProjectPreviewLifecycleOptions {
                workspace_root: workspace_root.to_path_buf(),
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/preview-stop") => {
            let body = preview_stop(ProjectPreviewLifecycleOptions {
                workspace_root: workspace_root.to_path_buf(),
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/repair-scaffold") => {
            let payload: RepairScaffoldRequest = if body.is_empty() {
                RepairScaffoldRequest { force: false }
            } else {
                read_json_body(body)?
            };
            let body = repair_project(ProjectRepairOptions {
                workspace_root: workspace_root.to_path_buf(),
                scaffold: true,
                force: payload.force,
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/feature-flow") => {
            let payload: FeatureFlowRequest = read_json_body(body)?;
            let body = feature_flow(ProjectFeatureFlowOptions {
                workspace_root: workspace_root.to_path_buf(),
                title: payload.title,
                description: payload.description,
                force: payload.force,
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/run-command") => {
            let payload: RunCommandRequest = read_json_body(body)?;
            let body = run_project_command(ProjectRunCommandOptions {
                workspace_root: workspace_root.to_path_buf(),
                kind: payload.kind,
                dry_run: payload.dry_run,
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/verify-generated") => {
            let body = verify_generated_project(ProjectVerifyGeneratedOptions {
                workspace_root: workspace_root.to_path_buf(),
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/execute-feature") => {
            let payload: ExecuteFeatureRequest = read_json_body(body)?;
            let body = execute_feature(ProjectExecuteFeatureOptions {
                workspace_root: workspace_root.to_path_buf(),
                feature_id: payload.feature_id,
                host: payload.host.unwrap_or(default_host),
                dry_run: payload.dry_run,
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        _ => Ok((
            404,
            "application/json".to_string(),
            json!({
                "ok": false,
                "status": "not_found",
                "path": path
            })
            .to_string(),
        )),
    }
}

fn read_json_body<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T> {
    let body = std::str::from_utf8(body).context("request body was not valid UTF-8")?;

    if body.trim().is_empty() {
        bail!("request body is required");
    }

    serde_json::from_str(&body).context("failed to parse request body as JSON")
}

fn split_url(url: &str) -> (String, String) {
    match url.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (url.to_string(), String::new()),
    }
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .filter_map(|part| part.split_once('='))
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, value)| value.replace('+', " "))
}

fn query_limit(query: &str) -> Option<usize> {
    if query.is_empty() {
        return None;
    }

    let mut limit = None;
    for part in query.split('&') {
        if let Some((key, value)) = part.split_once('=') {
            if key == "limit" {
                limit = value.parse::<usize>().ok();
            }
        }
    }
    limit
}

fn read_jsonl_tail(path: &Path, limit: usize) -> Result<Vec<serde_json::Value>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;

    let mut entries = Vec::new();
    for line in content
        .lines()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        entries.push(
            serde_json::from_str(trimmed)
                .with_context(|| format!("failed to parse log entry from {}", path.display()))?,
        );
    }
    Ok(entries)
}

fn read_text_tail(path: &Path, limit: usize) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(content
        .lines()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(str::to_string)
        .collect())
}

fn activity_feed(workspace_root: &Path, limit: usize) -> Result<serde_json::Value> {
    let inspect = inspect_project(ProjectInspectOptions {
        workspace_root: workspace_root.to_path_buf(),
    })?;
    let build_log_path = workspace_root.join(&inspect.capsule.state.build_log);
    let dispatch_log_path = workspace_root.join(".mutagen/state/dispatch-log.jsonl");
    let active_state_path = workspace_root.join(".mutagen/state/active-slice.json");

    let build_entries = read_jsonl_tail(&build_log_path, limit)?;
    let dispatch_entries = read_jsonl_tail(&dispatch_log_path, limit)?;
    let mut items = Vec::new();

    for entry in build_entries {
        items.push(json!({
            "kind": "build",
            "timestamp": entry.get("recorded_at").cloned().unwrap_or(serde_json::Value::Null),
            "title": format!(
                "{} {}",
                entry.get("command_kind").and_then(|value| value.as_str()).unwrap_or("command"),
                entry.get("status").and_then(|value| value.as_str()).unwrap_or("unknown")
            ),
            "detail": entry,
        }));
    }

    for entry in dispatch_entries {
        items.push(json!({
            "kind": "dispatch",
            "timestamp": entry.get("completed_at").cloned().unwrap_or(serde_json::Value::Null),
            "title": format!(
                "{} {}",
                entry.get("slice_id").and_then(|value| value.as_str()).unwrap_or("slice"),
                entry.get("status").and_then(|value| value.as_str()).unwrap_or("unknown")
            ),
            "detail": entry,
        }));
    }

    if active_state_path.exists() {
        let raw = fs::read_to_string(&active_state_path)
            .with_context(|| format!("failed to read {}", active_state_path.display()))?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", active_state_path.display()))?;
        items.push(json!({
            "kind": "active",
            "timestamp": parsed.get("started_at_unix_ms").cloned().unwrap_or(serde_json::Value::Null),
            "title": format!(
                "{} active at {}",
                parsed.get("slice_id").and_then(|value| value.as_str()).unwrap_or("slice"),
                parsed.get("stage").and_then(|value| value.as_str()).unwrap_or("unknown"),
            ),
            "detail": parsed,
        }));
    }

    items.sort_by(|left, right| {
        let left_key = sort_timestamp_key(left.get("timestamp"));
        let right_key = sort_timestamp_key(right.get("timestamp"));
        right_key.cmp(&left_key)
    });
    items.truncate(limit);

    Ok(json!({
        "ok": true,
        "status": "ready",
        "items": items,
        "build_log_path": build_log_path.to_string_lossy(),
        "dispatch_log_path": dispatch_log_path.to_string_lossy(),
        "active_state_path": active_state_path.to_string_lossy(),
    }))
}

fn sort_timestamp_key(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(value)) => value.clone(),
        Some(serde_json::Value::Number(value)) => {
            format!("{:020}", value.as_u64().unwrap_or_default())
        }
        _ => String::new(),
    }
}

fn slice_artifacts(workspace_root: &Path, slice_id: &str) -> Result<serde_json::Value> {
    let active_state_path = workspace_root.join(".mutagen/state/active-slice.json");
    let evidence_path = workspace_root
        .join(".mutagen/state/evidence")
        .join(format!("{slice_id}.md"));
    let review_dir = workspace_root.join("reviews").join(slice_id);
    let latest_qa_path = workspace_root.join(".mutagen/state/tiger-claw-latest.md");

    let active_state = if active_state_path.exists() {
        let raw = fs::read_to_string(&active_state_path)
            .with_context(|| format!("failed to read {}", active_state_path.display()))?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", active_state_path.display()))?;
        match parsed.get("slice_id").and_then(|value| value.as_str()) {
            Some(current) if current == slice_id => Some(parsed),
            _ => None,
        }
    } else {
        None
    };

    let evidence = read_optional_text(&evidence_path)?;
    let review_artifacts = read_markdown_files(&review_dir)?;
    let latest_qa = read_optional_text(&latest_qa_path)?;

    Ok(json!({
        "ok": true,
        "status": "ready",
        "slice_id": slice_id,
        "active_state_path": active_state_path.to_string_lossy(),
        "evidence": {
            "path": evidence_path.to_string_lossy(),
            "exists": evidence.is_some(),
            "body": evidence,
        },
        "active_state": active_state,
        "review_artifacts": review_artifacts,
        "latest_qa": {
            "path": latest_qa_path.to_string_lossy(),
            "exists": latest_qa.is_some(),
            "body": latest_qa,
        }
    }))
}

fn read_optional_text(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }

    Ok(Some(fs::read_to_string(path).with_context(|| {
        format!("failed to read {}", path.display())
    })?))
}

fn read_markdown_files(dir: &Path) -> Result<Vec<serde_json::Value>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut entries = fs::read_dir(dir)
        .with_context(|| format!("failed to read {}", dir.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    entries.sort();

    let mut artifacts = Vec::new();
    for path in entries {
        let body = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        artifacts.push(json!({
            "path": path.to_string_lossy(),
            "name": path.file_name().and_then(|value| value.to_str()).unwrap_or("artifact"),
            "body": body,
        }));
    }
    Ok(artifacts)
}

fn operate_slice_status(
    workspace_root: &Path,
    slice_id: &str,
    status: SliceStatus,
    reason: Option<String>,
) -> Result<serde_json::Value> {
    let reason = reason.unwrap_or_default();
    let escalation_reason = if reason.trim().is_empty() {
        None
    } else {
        Some(reason.trim().to_string())
    };
    let queue_path = workspace_root.join("slices/queue.json");
    let result = update_slice(UpdateSliceOptions {
        queue_path: queue_path.clone(),
        slice_id: slice_id.to_string(),
        status: Some(status),
        attempts: None,
        micro_corrections_used: None,
        karai_structural: None,
        bishop: None,
        tiger_claw: None,
        micro_correction: None,
        completed_at: None,
        clear_completed_at: false,
        escalation_reason,
        clear_escalation_reason: false,
        human_check_resolved_at: None,
        clear_human_check_resolved_at: false,
    })?;
    let active_state_cleared = clear_active_state_if_matches(workspace_root, slice_id)?;

    Ok(json!({
        "ok": true,
        "status": "updated",
        "operation": match status {
            SliceStatus::BlockedRetry => "mark_blocked",
            SliceStatus::Escalated => "escalate",
            SliceStatus::Refused => "refuse",
            SliceStatus::Completed => "completed",
            SliceStatus::InProgress => "in_progress",
            SliceStatus::Pending => "pending",
        },
        "result": result,
        "active_state_cleared": active_state_cleared,
        "queue_path": queue_path.to_string_lossy(),
    }))
}

fn resume_slice(workspace_root: &Path, slice_id: &str) -> Result<serde_json::Value> {
    let queue_path = workspace_root.join("slices/queue.json");
    let result = update_slice(UpdateSliceOptions {
        queue_path: queue_path.clone(),
        slice_id: slice_id.to_string(),
        status: Some(SliceStatus::Pending),
        attempts: None,
        micro_corrections_used: None,
        karai_structural: None,
        bishop: None,
        tiger_claw: None,
        micro_correction: None,
        completed_at: None,
        clear_completed_at: false,
        escalation_reason: None,
        clear_escalation_reason: true,
        human_check_resolved_at: None,
        clear_human_check_resolved_at: false,
    })?;

    Ok(json!({
        "ok": true,
        "status": "updated",
        "operation": "resume",
        "result": result,
        "queue_path": queue_path.to_string_lossy(),
    }))
}

fn queue_status(workspace_root: &Path) -> Result<serde_json::Value> {
    let queue_path = workspace_root.join("slices/queue.json");
    let queue = load_queue_file(&queue_path)?;
    let validation = validate_queue_file(&queue_path)?;
    let selection = queue.select_next_ready_slice();
    let active_state_path = workspace_root.join(".mutagen/state/active-slice.json");
    let active_slice_id = if active_state_path.exists() {
        let raw = fs::read_to_string(&active_state_path)
            .with_context(|| format!("failed to read {}", active_state_path.display()))?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", active_state_path.display()))?;
        parsed
            .get("slice_id")
            .and_then(|value| value.as_str())
            .map(str::to_string)
    } else {
        None
    };

    let summary = match selection {
        crate::queue::NextSliceSelection::Ready { index } => {
            let slice = &queue.slices[index];
            json!({
                "status": "ready",
                "next_ready_slice": {
                    "id": slice.id,
                    "title": slice.title,
                    "status": slice.status,
                },
                "blocked": [],
            })
        }
        crate::queue::NextSliceSelection::QueueClear => json!({
            "status": "queue_clear",
            "next_ready_slice": null,
            "blocked": [],
        }),
        crate::queue::NextSliceSelection::Stalled { blocked } => json!({
            "status": "stalled",
            "next_ready_slice": null,
            "blocked": blocked,
        }),
    };

    Ok(json!({
        "ok": validation.ok,
        "queue_path": queue_path.to_string_lossy(),
        "active_state_path": active_state_path.to_string_lossy(),
        "active_slice_id": active_slice_id,
        "validation": validation,
        "selection": summary,
    }))
}

fn clear_active_state_if_matches(workspace_root: &Path, slice_id: &str) -> Result<bool> {
    let active_state_path = workspace_root.join(".mutagen/state/active-slice.json");
    if !active_state_path.exists() {
        return Ok(false);
    }

    let raw = fs::read_to_string(&active_state_path)
        .with_context(|| format!("failed to read {}", active_state_path.display()))?;
    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", active_state_path.display()))?;
    let matches = parsed
        .get("slice_id")
        .and_then(|value| value.as_str())
        .map(|value| value == slice_id)
        .unwrap_or(false);

    if matches {
        fs::remove_file(&active_state_path)
            .with_context(|| format!("failed to remove {}", active_state_path.display()))?;
        return Ok(true);
    }

    Ok(false)
}

fn now_rfc3339() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format current timestamp")
}

fn dashboard_html() -> String {
    r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Mutagen Harness</title>
    <style>
      :root {
        color-scheme: light;
        --bg: #f3efe7;
        --bg-strong: #ebe1d2;
        --panel: rgba(255, 251, 244, 0.94);
        --ink: #1f1a16;
        --muted: #6b6258;
        --line: rgba(80, 62, 44, 0.18);
        --accent: #0f766e;
        --accent-strong: #115e59;
        --accent-soft: rgba(15, 118, 110, 0.11);
        --gold: #b45309;
        --gold-soft: rgba(180, 83, 9, 0.14);
        --danger: #b91c1c;
        --danger-soft: rgba(185, 28, 28, 0.08);
        --shadow: 0 22px 56px rgba(55, 42, 31, 0.08);
      }

      * { box-sizing: border-box; }

      body {
        margin: 0;
        min-height: 100vh;
        font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
        color: var(--ink);
        background:
          radial-gradient(circle at top left, rgba(15, 118, 110, 0.18), transparent 34%),
          radial-gradient(circle at top right, rgba(180, 83, 9, 0.14), transparent 30%),
          linear-gradient(180deg, #fbf7ef 0%, var(--bg) 100%);
      }

      main {
        width: min(1240px, calc(100% - 32px));
        margin: 0 auto;
        padding: 28px 0 44px;
      }

      .hero {
        display: grid;
        gap: 14px;
        margin-bottom: 20px;
      }

      .eyebrow {
        color: var(--muted);
        font-size: 0.84rem;
        letter-spacing: 0.08em;
        text-transform: uppercase;
      }

      .hero-top {
        display: flex;
        align-items: start;
        justify-content: space-between;
        gap: 18px;
      }

      .hero-copy {
        display: grid;
        gap: 12px;
      }

      .hero-copy h1 {
        margin: 0;
        font-size: clamp(2.5rem, 5vw, 4.2rem);
        line-height: 0.95;
        font-family: "IBM Plex Serif", Georgia, serif;
        font-weight: 600;
      }

      .hero-copy p {
        margin: 0;
        max-width: 720px;
        color: var(--muted);
        font-size: 1.02rem;
      }

      .hero-actions {
        display: flex;
        align-items: center;
        gap: 10px;
        flex-wrap: wrap;
      }

      .grid {
        display: grid;
        gap: 16px;
        grid-template-columns: minmax(0, 1.3fr) minmax(360px, 0.9fr);
      }

      .panel {
        background: var(--panel);
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 18px;
        box-shadow: var(--shadow);
      }

      .panel h2 {
        margin: 0;
        font-size: 1rem;
      }

      .panel-header {
        display: flex;
        align-items: start;
        justify-content: space-between;
        gap: 12px;
        margin-bottom: 14px;
      }

      .panel-header p {
        margin: 6px 0 0;
        color: var(--muted);
        font-size: 0.92rem;
      }

      .stats {
        display: grid;
        gap: 10px;
        grid-template-columns: repeat(4, minmax(0, 1fr));
      }

      .stat {
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 12px;
        background: rgba(255, 255, 255, 0.5);
      }

      .stat strong {
        display: block;
        font-size: 1.4rem;
        margin-top: 4px;
      }

      .label {
        color: var(--muted);
        font-size: 0.82rem;
        text-transform: uppercase;
        letter-spacing: 0.04em;
      }

      .row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
      }

      .pill {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        border: 1px solid var(--line);
        border-radius: 999px;
        padding: 4px 10px;
        font-size: 0.82rem;
        background: rgba(255, 255, 255, 0.65);
      }

      .pill.attention { color: var(--gold); }
      .pill.ready, .pill.in_progress, .pill.feature_slice_ready { color: var(--accent-strong); }
      .pill.complete { color: var(--accent); }
      .pill.queued, .pill.planned, .pill.not_enqueued { color: var(--muted); }
      .pill.blocked_retry, .pill.escalated, .pill.refused, .pill.error { color: var(--danger); }

      .stack {
        display: grid;
        gap: 10px;
      }

      .mini-grid {
        display: grid;
        gap: 10px;
        grid-template-columns: repeat(2, minmax(0, 1fr));
      }

      .kv {
        display: grid;
        gap: 6px;
      }

      .kv-item {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 12px;
        padding: 8px 0;
        border-bottom: 1px solid var(--line);
      }

      .kv-item:last-child {
        border-bottom: 0;
      }

      .muted {
        color: var(--muted);
      }

      form {
        display: grid;
        gap: 10px;
      }

      input, textarea {
        width: 100%;
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 11px 12px;
        background: rgba(255, 255, 255, 0.78);
        color: var(--ink);
        font: inherit;
      }

      textarea {
        min-height: 96px;
        resize: vertical;
      }

      button {
        border: 0;
        border-radius: 8px;
        padding: 11px 14px;
        background: var(--accent);
        color: white;
        font: inherit;
        font-weight: 600;
        cursor: pointer;
      }

      button.secondary {
        background: #e5ddd0;
        color: var(--ink);
      }

      button.warn {
        background: var(--gold);
      }

      table {
        width: 100%;
        border-collapse: collapse;
      }

      th, td {
        text-align: left;
        padding: 10px 0;
        border-bottom: 1px solid var(--line);
        font-size: 0.92rem;
      }

      tbody tr:hover td {
        background: rgba(255, 255, 255, 0.42);
      }

      pre {
        margin: 0;
        padding: 14px;
        border-radius: 8px;
        border: 1px solid var(--line);
        background: #1d1f21;
        color: #f3efe6;
        font-family: "IBM Plex Mono", "Cascadia Code", monospace;
        overflow: auto;
      }

      .full {
        grid-column: 1 / -1;
      }

      .feature-list {
        display: grid;
        gap: 10px;
      }

      .feature-card {
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 12px;
        background: rgba(255, 255, 255, 0.52);
        display: grid;
        gap: 8px;
      }

      .feature-card button {
        justify-self: start;
      }

      .timeline {
        display: grid;
        gap: 10px;
      }

      .timeline-item {
        display: grid;
        gap: 6px;
        padding: 12px;
        border-radius: 8px;
        border: 1px solid var(--line);
        background: rgba(255, 255, 255, 0.46);
      }

      .timeline-item.active {
        border-color: rgba(15, 118, 110, 0.35);
        background: var(--accent-soft);
      }

      .timeline-item.blocked {
        border-color: rgba(185, 28, 28, 0.25);
        background: var(--danger-soft);
      }

      .timeline-item.complete {
        border-color: rgba(15, 118, 110, 0.22);
      }

      .callout {
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 12px;
        background: rgba(255, 255, 255, 0.56);
      }

      .callout strong {
        display: block;
        margin-bottom: 6px;
      }

      .actions {
        display: flex;
        gap: 10px;
        flex-wrap: wrap;
      }

      .actions button {
        min-width: 140px;
      }

      .hidden {
        display: none !important;
      }

      @media (max-width: 900px) {
        .grid { grid-template-columns: 1fr; }
        .stats { grid-template-columns: repeat(2, minmax(0, 1fr)); }
        .hero-top { grid-template-columns: 1fr; display: grid; }
        .mini-grid { grid-template-columns: 1fr; }
        .actions button { width: 100%; }
      }
    </style>
  </head>
  <body>
    <main>
      <section class="hero">
        <div class="hero-top">
          <div class="hero-copy">
            <div class="eyebrow">Local Control Plane</div>
            <h1>Mutagen Harness</h1>
            <p>One surface for project health, feature intake, execution progress, and the next move without rummaging through queue JSON.</p>
          </div>
          <div class="hero-actions">
            <button id="refresh" class="secondary" type="button">Refresh Snapshot</button>
          </div>
        </div>
      </section>

      <section class="grid">
        <article class="panel">
          <div class="panel-header">
            <div>
              <h2>Project snapshot</h2>
              <p>Health, preview state, and the shape of the current workspace.</p>
            </div>
          </div>
          <div id="project-summary" class="stack"></div>
        </article>

        <article class="panel">
          <div class="panel-header">
            <div>
              <h2>Feature intake</h2>
              <p>Run the full backside flow in one pass and push executable work into the queue.</p>
            </div>
          </div>
          <form id="feature-form">
            <input id="feature-title" name="title" placeholder="Add due dates" required>
            <textarea id="feature-description" name="description" placeholder="Tasks should include optional due dates."></textarea>
            <button type="submit">Queue Feature Flow</button>
          </form>
        </article>

        <article class="panel full">
          <div class="panel-header">
            <div>
              <h2>Bootstrap health</h2>
              <p>Tooling checks, missing scaffold paths, and the shortest path from "half-built" to usable.</p>
            </div>
          </div>
          <div id="bootstrap-health-panel" class="stack"></div>
        </article>

        <article class="panel full">
          <div class="panel-header">
            <div>
              <h2>Preview and build</h2>
              <p>Start or stop preview, poke readiness, run setup/test/build, and keep the latest result in view.</p>
            </div>
          </div>
          <div id="preview-build-panel" class="stack"></div>
        </article>

        <article class="panel full">
          <div class="panel-header">
            <div>
              <h2>Debug detail</h2>
              <p>Recent build history and preview log output, because vibes are not a debugging strategy.</p>
            </div>
          </div>
          <div id="debug-detail-panel" class="stack"></div>
        </article>

        <article class="panel full">
          <div class="panel-header">
            <div>
              <h2>Queue control</h2>
              <p>See what can move next, what is blocked, and nudge the queue forward without guessing.</p>
            </div>
          </div>
          <div id="queue-control-panel" class="stack"></div>
        </article>

        <article class="panel full">
          <div class="panel-header">
            <div>
              <h2>Activity feed</h2>
              <p>Recent build, dispatch, and active-slice events in one timeline so the console remembers what just happened.</p>
            </div>
          </div>
          <div id="activity-feed-panel" class="stack"></div>
        </article>

        <article class="panel full">
          <div class="panel-header">
            <div>
              <h2>Feature backlog</h2>
              <p>Everything waiting in the backside lane, from raw requests to queued execution slices.</p>
            </div>
            <div id="feature-counts" class="actions"></div>
          </div>
          <table>
            <thead>
              <tr>
                <th>Feature</th>
                <th>Status</th>
                <th>Created</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody id="feature-table"></tbody>
          </table>
        </article>

        <article class="panel">
          <div class="panel-header">
            <div>
              <h2>Active feature</h2>
              <p>The feature currently holding the baton, if any.</p>
            </div>
          </div>
          <div id="active-feature-panel" class="stack"></div>
        </article>

        <article class="panel">
          <div class="panel-header">
            <div>
              <h2>Feature detail</h2>
              <p>Slice-by-slice progress for whichever feature you select from the backlog.</p>
            </div>
          </div>
          <div id="feature-detail-panel" class="stack"></div>
        </article>

        <article class="panel full">
          <div class="panel-header">
            <div>
              <h2>Slice artifacts</h2>
              <p>Evidence bundle, review artifacts, and active-state snapshot for the slice you are supervising.</p>
            </div>
          </div>
          <div id="slice-artifacts-panel" class="stack"></div>
        </article>

        <article class="panel full">
          <div class="panel-header">
            <div>
              <h2>Last response</h2>
              <p>Wire output from the most recent action, handy when the machine is being a little too honest.</p>
            </div>
          </div>
          <pre id="response-log">{}</pre>
        </article>
      </section>
    </main>

    <script>
      let dashboardState = null;
      let selectedFeatureId = null;

      const responseLog = document.getElementById("response-log");
      const projectSummary = document.getElementById("project-summary");
      const featureCounts = document.getElementById("feature-counts");
      const featureTable = document.getElementById("feature-table");
      const activeFeaturePanel = document.getElementById("active-feature-panel");
      const featureDetailPanel = document.getElementById("feature-detail-panel");
      const previewBuildPanel = document.getElementById("preview-build-panel");
      const debugDetailPanel = document.getElementById("debug-detail-panel");
      const queueControlPanel = document.getElementById("queue-control-panel");
      const activityFeedPanel = document.getElementById("activity-feed-panel");
      const bootstrapHealthPanel = document.getElementById("bootstrap-health-panel");
      const sliceArtifactsPanel = document.getElementById("slice-artifacts-panel");

      function setLog(value) {
        responseLog.textContent = JSON.stringify(value, null, 2);
      }

      function badge(status) {
        return `<span class="pill ${status}">${status.replaceAll("_", " ")}</span>`;
      }

      function escapeHtml(value) {
        return String(value)
          .replaceAll("&", "&amp;")
          .replaceAll("<", "&lt;")
          .replaceAll(">", "&gt;")
          .replaceAll("\"", "&quot;")
          .replaceAll("'", "&#39;");
      }

      function projectCard(data, previewPlan) {
        const preview = data.project.preview;
        const active = data.active_feature;
        const previewUrl = preview.url || (previewPlan.ok ? previewPlan.url : "");
        return `
          <div class="stats">
            <div class="stat"><span class="label">Project</span><strong>${escapeHtml(data.project.stack)}</strong>${badge(data.status)}</div>
            <div class="stat"><span class="label">Features</span><strong>${data.feature_backlog.total}</strong>${data.feature_backlog.in_queue} in queue</div>
            <div class="stat"><span class="label">Preview</span><strong>${escapeHtml(preview.status)}</strong>${previewUrl ? `<a href="${escapeHtml(previewUrl)}" target="_blank" rel="noreferrer">${escapeHtml(previewUrl)}</a>` : "No URL"}</div>
            <div class="stat"><span class="label">Active</span><strong>${active ? escapeHtml(active.feature.title) : "None"}</strong>${active ? badge(active.status) : "Idle"}</div>
          </div>
          <div class="mini-grid">
            <div class="callout">
              <strong>Scaffold health</strong>
              <div class="kv">
                <div class="kv-item"><span class="muted">Capsule</span><span>${data.project.capsule_ok ? "ok" : "missing"}</span></div>
                <div class="kv-item"><span class="muted">Scaffold</span><span>${data.project.scaffold_ok ? "ok" : "attention"}</span></div>
                <div class="kv-item"><span class="muted">Doctor</span><span>${data.project.doctor_ok ? "ok" : "missing tools"}</span></div>
              </div>
            </div>
            <div class="callout">
              <strong>Recent build</strong>
              ${
                data.project.last_build_log_entry
                  ? `<div class="kv">
                      <div class="kv-item"><span class="muted">Status</span><span>${escapeHtml(data.project.last_build_log_entry.status || "unknown")}</span></div>
                      <div class="kv-item"><span class="muted">Kind</span><span>${escapeHtml(data.project.last_build_log_entry.command_kind || "unknown")}</span></div>
                      <div class="kv-item"><span class="muted">Recorded</span><span>${escapeHtml(data.project.last_build_log_entry.recorded_at || "unknown")}</span></div>
                    </div>`
                  : `<div class="muted">No build log entries yet.</div>`
              }
            </div>
          </div>
        `;
      }

      function renderBootstrapHealth(data) {
        const doctor = data.project.doctor || { checks: [] };
        const missingScaffold = data.project.missing_scaffold_paths || [];
        const missingCapsule = data.project.missing_paths || [];

        bootstrapHealthPanel.innerHTML = `
          <div class="mini-grid">
            <div class="callout">
              <strong>Workspace health</strong>
              <div class="kv">
                <div class="kv-item"><span class="muted">Doctor</span><span>${data.project.doctor_ok ? "ok" : "attention"}</span></div>
                <div class="kv-item"><span class="muted">Missing scaffold</span><span>${missingScaffold.length}</span></div>
                <div class="kv-item"><span class="muted">Missing capsule paths</span><span>${missingCapsule.length}</span></div>
              </div>
              <div class="actions">
                <button type="button" class="secondary" id="bootstrap-run-doctor">Run Doctor</button>
                <button type="button" class="secondary" id="bootstrap-run-setup">Run Setup</button>
                <button type="button" class="warn" id="bootstrap-repair-scaffold">Repair Scaffold</button>
              </div>
            </div>
            <div class="callout">
              <strong>Doctor detail</strong>
              ${
                doctor.checks && doctor.checks.length > 0
                  ? `<div class="kv">
                      ${doctor.checks.map((check) => `
                        <div class="kv-item">
                          <span class="muted">${escapeHtml(check.executable)}</span>
                          <span>${escapeHtml(check.status)}${check.detail ? ` - ${escapeHtml(check.detail)}` : ""}</span>
                        </div>
                      `).join("")}
                    </div>`
                  : `<div class="muted">No doctor data yet.</div>`
              }
            </div>
          </div>
          <div class="mini-grid">
            <div class="callout">
              <strong>Missing scaffold paths</strong>
              ${
                missingScaffold.length > 0
                  ? `<div class="kv">
                      ${missingScaffold.map((path) => `<div class="kv-item"><span>${escapeHtml(path)}</span></div>`).join("")}
                    </div>`
                  : `<div class="muted">No scaffold files are currently missing.</div>`
              }
            </div>
            <div class="callout">
              <strong>Missing capsule paths</strong>
              ${
                missingCapsule.length > 0
                  ? `<div class="kv">
                      ${missingCapsule.map((path) => `<div class="kv-item"><span>${escapeHtml(path)}</span></div>`).join("")}
                    </div>`
                  : `<div class="muted">No required capsule-managed paths are missing.</div>`
              }
            </div>
          </div>
        `;

        document.getElementById("bootstrap-run-doctor").addEventListener("click", async () => {
          await runAction(async () => fetchJson("/api/doctor"));
        });

        document.getElementById("bootstrap-run-setup").addEventListener("click", async () => {
          await runAction(async () => postJson("/api/run-command", { kind: "setup" }));
        });

        document.getElementById("bootstrap-repair-scaffold").addEventListener("click", async () => {
          await runAction(async () => postJson("/api/repair-scaffold", {}));
        });
      }

      function renderPreviewBuild(data, previewPlan) {
        const preview = data.project.preview;
        const build = data.project.last_build_log_entry;
        const previewUrl = preview.url || (previewPlan.ok ? previewPlan.url : "");
        const previewCommand = preview.command || (previewPlan.ok ? previewPlan.command : "");
        const previewKind = previewPlan.ok ? previewPlan.command_kind : "unknown";
        const timeout = previewPlan.ok ? previewPlan.readiness_timeout_seconds : null;
        const toolChecks = (data.project.doctor && data.project.doctor.checks) ? data.project.doctor.checks : [];

        previewBuildPanel.innerHTML = `
          <div class="mini-grid">
            <div class="callout">
              <strong>Preview control</strong>
              <div class="kv">
                <div class="kv-item"><span class="muted">Status</span><span>${badge(preview.status)}</span></div>
                <div class="kv-item"><span class="muted">Running</span><span>${preview.running ? "yes" : "no"}</span></div>
                <div class="kv-item"><span class="muted">Ready</span><span>${preview.ready ? "yes" : "no"}</span></div>
                <div class="kv-item"><span class="muted">URL</span><span>${previewUrl ? `<a href="${escapeHtml(previewUrl)}" target="_blank" rel="noreferrer">${escapeHtml(previewUrl)}</a>` : "Not configured"}</span></div>
                <div class="kv-item"><span class="muted">Command</span><span>${previewCommand ? escapeHtml(previewCommand) : "Not configured"}</span></div>
                <div class="kv-item"><span class="muted">Kind</span><span>${escapeHtml(String(previewKind).replaceAll("_", " "))}</span></div>
                <div class="kv-item"><span class="muted">Timeout</span><span>${timeout === null ? "Unknown" : `${timeout}s`}</span></div>
              </div>
              <div class="actions">
                <button type="button" data-preview-action="start">Start Preview</button>
                <button type="button" class="secondary" data-preview-action="check">Check Preview</button>
                <button type="button" class="secondary" data-preview-action="stop">Stop Preview</button>
                <button type="button" class="secondary" data-debug-view="preview-log">Open Preview Log</button>
              </div>
            </div>
            <div class="callout">
              <strong>Build control</strong>
              ${
                build
                  ? `<div class="kv">
                      <div class="kv-item"><span class="muted">Last status</span><span>${escapeHtml(build.status || "unknown")}</span></div>
                      <div class="kv-item"><span class="muted">Command kind</span><span>${escapeHtml(build.command_kind || "unknown")}</span></div>
                      <div class="kv-item"><span class="muted">Exit code</span><span>${escapeHtml(String(build.exit_code ?? "none"))}</span></div>
                      <div class="kv-item"><span class="muted">Recorded</span><span>${escapeHtml(build.recorded_at || "unknown")}</span></div>
                    </div>`
                  : `<div class="muted">No build log entries yet. The harness is being polite and waiting for a command.</div>`
              }
              <div class="actions">
                <button type="button" class="secondary" data-command-kind="setup">Run Setup</button>
                <button type="button" class="secondary" data-command-kind="test">Run Test</button>
                <button type="button" class="secondary" data-command-kind="build">Run Build</button>
                <button type="button" class="warn" data-action="verify-generated">Verify Generated</button>
                <button type="button" class="secondary" data-debug-view="build-log">Open Build History</button>
              </div>
            </div>
          </div>
          <div class="callout">
            <strong>Tooling reality check</strong>
            ${
              toolChecks.length === 0
                ? `<div class="muted">No doctor data is available yet.</div>`
                : `<div class="kv">
                    ${toolChecks.map((check) => `
                      <div class="kv-item">
                        <span class="muted">${escapeHtml(check.executable)}</span>
                        <span>${escapeHtml(check.status)}${check.detail ? ` - ${escapeHtml(check.detail)}` : ""}</span>
                      </div>
                    `).join("")}
                  </div>`
            }
          </div>
        `;

        previewBuildPanel.querySelectorAll("button[data-preview-action]").forEach((button) => {
          button.addEventListener("click", async () => {
            const action = button.dataset.previewAction;
            await runAction(async () => {
              if (action === "check") {
                return fetchJson("/api/preview-check");
              }
              if (action === "start") {
                return postJson("/api/preview-start", {});
              }
              return postJson("/api/preview-stop", {});
            });
          });
        });

        previewBuildPanel.querySelectorAll("button[data-command-kind]").forEach((button) => {
          button.addEventListener("click", async () => {
            await runAction(async () => postJson("/api/run-command", {
              kind: button.dataset.commandKind
            }));
          });
        });

        const verifyButton = previewBuildPanel.querySelector("button[data-action='verify-generated']");
        if (verifyButton) {
          verifyButton.addEventListener("click", async () => {
            await runAction(async () => postJson("/api/verify-generated", {}));
          });
        }

        previewBuildPanel.querySelectorAll("button[data-debug-view]").forEach((button) => {
          button.addEventListener("click", async () => {
            if (button.dataset.debugView === "build-log") {
              await loadBuildLog();
              return;
            }
            await loadPreviewLog();
          });
        });
      }

      function renderDebugDetail(state) {
        const buildEntries = state.buildLog?.entries || [];
        const previewLines = state.previewLog?.lines || [];
        const buildBody = buildEntries.length === 0
          ? `<div class="muted">No build history yet.</div>`
          : `<pre>${escapeHtml(JSON.stringify(buildEntries, null, 2))}</pre>`;
        const previewBody = previewLines.length === 0
          ? `<div class="muted">Preview log is empty.</div>`
          : `<pre>${escapeHtml(previewLines.join("\n"))}</pre>`;

        debugDetailPanel.innerHTML = `
          <div class="mini-grid">
            <div class="callout">
              <strong>Build history</strong>
              <div class="actions">
                <button type="button" class="secondary" id="debug-refresh-build">Refresh Build History</button>
              </div>
              ${buildBody}
            </div>
            <div class="callout">
              <strong>Preview log tail</strong>
              <div class="actions">
                <button type="button" class="secondary" id="debug-refresh-preview">Refresh Preview Log</button>
              </div>
              ${previewBody}
            </div>
          </div>
        `;

        document.getElementById("debug-refresh-build").addEventListener("click", async () => {
          await loadBuildLog();
        });

        document.getElementById("debug-refresh-preview").addEventListener("click", async () => {
          await loadPreviewLog();
        });
      }

      async function loadBuildLog() {
        const buildLog = await safeFetchJson("/api/build-log?limit=12");
        dashboardState = {
          ...(dashboardState || {}),
          buildLog
        };
        renderDebugDetail(dashboardState);
        setLog(buildLog);
      }

      async function loadPreviewLog() {
        const previewLog = await safeFetchJson("/api/preview-log?limit=80");
        dashboardState = {
          ...(dashboardState || {}),
          previewLog
        };
        renderDebugDetail(dashboardState);
        setLog(previewLog);
      }

      function renderQueueControl(state) {
        const queue = state.queueStatus || { selection: { status: "unknown", blocked: [] }, validation: { issues: [] } };
        const selection = queue.selection || {};
        const blocked = selection.blocked || [];
        const issues = queue.validation?.issues || [];
        const nextReady = selection.next_ready_slice;

        queueControlPanel.innerHTML = `
          <div class="mini-grid">
            <div class="callout">
              <strong>Queue selection</strong>
              <div class="kv">
                <div class="kv-item"><span class="muted">Status</span><span>${badge(selection.status || "unknown")}</span></div>
                <div class="kv-item"><span class="muted">Active slice</span><span>${escapeHtml(queue.active_slice_id || "none")}</span></div>
                <div class="kv-item"><span class="muted">Validation</span><span>${queue.validation?.ok ? "ok" : "attention"}</span></div>
              </div>
              ${
                nextReady
                  ? `<div class="callout">
                      <strong>Next ready slice</strong>
                      <div class="muted">${escapeHtml(nextReady.id)}</div>
                      <div>${escapeHtml(nextReady.title || "")}</div>
                    </div>`
                  : `<div class="muted">No ready slice is currently selectable.</div>`
              }
              <div class="actions">
                <button type="button" class="secondary" id="queue-refresh">Refresh Queue</button>
                <button type="button" id="queue-prepare-next">Prepare Next Ready Slice</button>
              </div>
            </div>
            <div class="callout">
              <strong>Blocked slices</strong>
              ${
                blocked.length === 0
                  ? `<div class="muted">Nothing is blocked right now.</div>`
                  : blocked.map((entry) => `
                      <div class="feature-card">
                        <strong>${escapeHtml(entry.id)}</strong>
                        <div class="muted">${escapeHtml((entry.unmet_dependencies || []).join(", ") || "no dependency data")}</div>
                        <div class="actions">
                          <button type="button" class="secondary" data-queue-open-slice="${entry.id}">Inspect Slice</button>
                          <button type="button" class="secondary" data-queue-resume-slice="${entry.id}">Resume Slice</button>
                        </div>
                      </div>
                    `).join("")
              }
            </div>
          </div>
          <div class="callout">
            <strong>Validation issues</strong>
            ${
              issues.length === 0
                ? `<div class="muted">Queue validation is clean.</div>`
                : `<div class="timeline">
                    ${issues.map((issue) => `
                      <div class="timeline-item ${issue.level === "error" ? "blocked" : ""}">
                        <div class="row">
                          <strong>${escapeHtml(issue.code)}</strong>
                          ${badge(issue.level)}
                        </div>
                        <div class="muted">${escapeHtml(issue.message)}</div>
                        <div class="muted">${escapeHtml(issue.slice_id || issue.advisory_id || "")}</div>
                      </div>
                    `).join("")}
                  </div>`
            }
          </div>
        `;

        document.getElementById("queue-refresh").addEventListener("click", async () => {
          await loadQueueStatus();
        });

        document.getElementById("queue-prepare-next").addEventListener("click", async () => {
          await runAction(async () => postJson("/api/queue-prepare-next", {}));
          await loadQueueStatus();
        });

        queueControlPanel.querySelectorAll("[data-queue-open-slice]").forEach((button) => {
          button.addEventListener("click", async () => {
            await loadSliceArtifacts(button.dataset.queueOpenSlice);
          });
        });

        queueControlPanel.querySelectorAll("[data-queue-resume-slice]").forEach((button) => {
          button.addEventListener("click", async () => {
            await runAction(async () => postJson("/api/slice-resume", {
              slice_id: button.dataset.queueResumeSlice,
              reason: ""
            }));
            await loadQueueStatus();
          });
        });
      }

      async function loadQueueStatus() {
        const queueStatus = await safeFetchJson("/api/queue-status");
        dashboardState = {
          ...(dashboardState || {}),
          queueStatus
        };
        renderQueueControl(dashboardState);
        setLog(queueStatus);
      }

      function renderActivityFeed(state) {
        const items = state.activityFeed?.items || [];
        activityFeedPanel.innerHTML = `
          <div class="actions">
            <button type="button" class="secondary" id="activity-refresh">Refresh Activity</button>
          </div>
          ${
            items.length === 0
              ? `<div class="callout"><strong>No activity yet.</strong><div class="muted">Once the harness starts doing things, the receipts will show up here.</div></div>`
              : `<div class="timeline">
                  ${items.map((item) => `
                    <div class="timeline-item ${item.kind === "dispatch" ? "complete" : item.kind === "active" ? "active" : ""}">
                      <div class="row">
                        <strong>${escapeHtml(item.title || item.kind || "event")}</strong>
                        ${badge(item.kind || "event")}
                      </div>
                      <div class="muted">${escapeHtml(String(item.timestamp ?? "unknown"))}</div>
                      <pre>${escapeHtml(JSON.stringify(item.detail, null, 2))}</pre>
                    </div>
                  `).join("")}
                </div>`
          }
        `;

        document.getElementById("activity-refresh").addEventListener("click", async () => {
          await loadActivityFeed();
        });
      }

      async function loadActivityFeed() {
        const activityFeed = await safeFetchJson("/api/activity-feed?limit=16");
        dashboardState = {
          ...(dashboardState || {}),
          activityFeed
        };
        renderActivityFeed(dashboardState);
        setLog(activityFeed);
      }

      function renderSliceArtifacts(state) {
        const artifacts = state.sliceArtifacts || { review_artifacts: [], evidence: {}, latest_qa: {} };
        const evidenceBody = artifacts.evidence?.body
          ? `<pre>${escapeHtml(artifacts.evidence.body)}</pre>`
          : `<div class="muted">No evidence bundle found for this slice yet.</div>`;
        const activeStateBody = artifacts.active_state
          ? `<pre>${escapeHtml(JSON.stringify(artifacts.active_state, null, 2))}</pre>`
          : `<div class="muted">This slice is not the currently claimed slice, so there is no live active-state snapshot to show.</div>`;
        const reviewBody = artifacts.review_artifacts && artifacts.review_artifacts.length > 0
          ? artifacts.review_artifacts.map((artifact) => `
              <div class="callout">
                <strong>${escapeHtml(artifact.name || "artifact")}</strong>
                <div class="muted">${escapeHtml(artifact.path || "")}</div>
                <pre>${escapeHtml(artifact.body || "")}</pre>
              </div>
            `).join("")
          : `<div class="muted">No review artifacts have been written for this slice yet.</div>`;
        const latestQaBody = artifacts.latest_qa?.body
          ? `<pre>${escapeHtml(artifacts.latest_qa.body)}</pre>`
          : `<div class="muted">No latest QA snapshot is available.</div>`;

        sliceArtifactsPanel.innerHTML = `
          <div class="callout">
            <strong>Operator controls</strong>
            <div class="muted">${escapeHtml(artifacts.slice_id || "No slice selected")}</div>
            <div class="actions">
              <button type="button" class="secondary" id="slice-refresh-artifacts">Refresh Active State</button>
              <button type="button" class="secondary" id="slice-mark-blocked">Mark Blocked</button>
              <button type="button" class="secondary" id="slice-escalate">Escalate</button>
              <button type="button" class="warn" id="slice-finalize">Finalize Slice</button>
            </div>
          </div>
          <div class="mini-grid">
            <div class="callout">
              <strong>Evidence bundle</strong>
              <div class="muted">${escapeHtml(artifacts.evidence?.path || "")}</div>
              ${evidenceBody}
            </div>
            <div class="callout">
              <strong>Active state</strong>
              <div class="muted">${escapeHtml(artifacts.active_state_path || "")}</div>
              ${activeStateBody}
            </div>
          </div>
          <div class="mini-grid">
            <div class="callout">
              <strong>Review artifacts</strong>
              ${reviewBody}
            </div>
            <div class="callout">
              <strong>Latest QA snapshot</strong>
              <div class="muted">${escapeHtml(artifacts.latest_qa?.path || "")}</div>
              ${latestQaBody}
            </div>
          </div>
        `;

        document.getElementById("slice-refresh-artifacts").addEventListener("click", async () => {
          await loadSliceArtifacts(artifacts.slice_id);
        });

        document.getElementById("slice-mark-blocked").addEventListener("click", async () => {
          await runSliceOperator("/api/slice-mark-blocked", artifacts.slice_id, "Paused from dashboard");
        });

        document.getElementById("slice-escalate").addEventListener("click", async () => {
          await runSliceOperator("/api/slice-escalate", artifacts.slice_id, "Escalated from dashboard");
        });

        document.getElementById("slice-finalize").addEventListener("click", async () => {
          await runSliceOperator("/api/slice-finalize", artifacts.slice_id, "");
        });
      }

      async function loadSliceArtifacts(sliceId) {
        if (!sliceId) {
          dashboardState = {
            ...(dashboardState || {}),
            sliceArtifacts: null
          };
          sliceArtifactsPanel.innerHTML = `
            <div class="callout">
              <strong>No slice selected.</strong>
              <div class="muted">Select a slice from feature detail or open the active slice to inspect its artifacts.</div>
            </div>
          `;
          return;
        }

        const sliceArtifacts = await safeFetchJson(`/api/slice-artifacts?slice_id=${encodeURIComponent(sliceId)}`);
        dashboardState = {
          ...(dashboardState || {}),
          sliceArtifacts
        };
        renderSliceArtifacts(dashboardState);
      }

      async function runSliceOperator(url, sliceId, reason) {
        await runAction(async () => postJson(url, {
          slice_id: sliceId,
          reason
        }));
        await loadSliceArtifacts(sliceId);
      }

      function renderBacklog(data) {
        featureCounts.innerHTML = `
          <span class="pill">queued ${data.feature_backlog.queued}</span>
          <span class="pill">planned ${data.feature_backlog.planned}</span>
          <span class="pill">ready ${data.feature_backlog.ready}</span>
          <span class="pill">in queue ${data.feature_backlog.in_queue}</span>
        `;

        if (!selectedFeatureId && data.feature_backlog.features.length > 0) {
          selectedFeatureId = data.feature_backlog.features[0].id;
        }

        featureTable.innerHTML = data.feature_backlog.features.map((feature) => `
          <tr data-feature-id="${feature.id}">
            <td>${escapeHtml(feature.title)}<br><span class="label">${escapeHtml(feature.id)}</span></td>
            <td>${badge(feature.status)}</td>
            <td>${escapeHtml(feature.created_at)}</td>
            <td><button type="button" data-action="execute" data-feature-id="${feature.id}">Execute Next</button></td>
          </tr>
        `).join("");

        featureTable.querySelectorAll("tr[data-feature-id]").forEach((row) => {
          row.addEventListener("click", async (event) => {
            if (event.target.closest("button")) {
              return;
            }
            selectedFeatureId = row.dataset.featureId;
            await loadFeatureDetail(selectedFeatureId);
          });
        });

        featureTable.querySelectorAll("button[data-action='execute']").forEach((button) => {
          button.addEventListener("click", async () => {
            const payload = await postJson("/api/execute-feature", {
              feature_id: button.dataset.featureId
            });
            setLog(payload);
            await refreshDashboard();
          });
        });
      }

      function renderActiveFeature(data) {
        const active = data.active_feature;

        if (!active) {
          activeFeaturePanel.innerHTML = `
            <div class="callout">
              <strong>Nothing is currently claimed.</strong>
              <div class="muted">Once a feature slice is prepared, its live progress and active agent will show up here.</div>
            </div>
          `;
          return;
        }

        activeFeaturePanel.innerHTML = `
          <div class="feature-card">
            <div class="row">
              <strong>${escapeHtml(active.feature.title)}</strong>
              ${badge(active.status)}
            </div>
            <div class="muted">${escapeHtml(active.feature.id)}</div>
            ${
              active.active_slice
                ? `<div class="kv">
                    <div class="kv-item"><span class="muted">Slice</span><span>${escapeHtml(active.active_slice.id)}</span></div>
                    <div class="kv-item"><span class="muted">Stage</span><span>${escapeHtml(active.active_slice.stage)}</span></div>
                    <div class="kv-item"><span class="muted">Agent</span><span>${escapeHtml(active.active_slice.active_agent)}</span></div>
                  </div>`
                : `<div class="muted">No active slice metadata yet.</div>`
            }
            <div class="actions">
              <button type="button" id="active-execute">Advance Feature</button>
              <button type="button" class="secondary" id="active-open-detail">Open Detail</button>
              ${active.active_slice ? `<button type="button" class="secondary" id="active-open-artifacts">Open Slice Artifacts</button>` : ""}
            </div>
          </div>
        `;

        document.getElementById("active-execute").addEventListener("click", async () => {
          const payload = await postJson("/api/execute-feature", {
            feature_id: active.feature.id
          });
          setLog(payload);
          await refreshDashboard();
        });

        document.getElementById("active-open-detail").addEventListener("click", async () => {
          selectedFeatureId = active.feature.id;
          await loadFeatureDetail(active.feature.id);
        });

        const openArtifacts = document.getElementById("active-open-artifacts");
        if (openArtifacts) {
          openArtifacts.addEventListener("click", async () => {
            await loadSliceArtifacts(active.active_slice.id);
          });
        }
      }

      function timelineItemClass(slice) {
        if (slice.status === "in_progress") return "timeline-item active";
        if (slice.status === "blocked_retry" || slice.status === "escalated" || slice.status === "refused") {
          return "timeline-item blocked";
        }
        if (slice.status === "completed") return "timeline-item complete";
        return "timeline-item";
      }

      async function loadFeatureDetail(featureId) {
        if (!featureId) {
          featureDetailPanel.innerHTML = `
            <div class="callout">
              <strong>No feature selected.</strong>
              <div class="muted">Choose a feature from the backlog to inspect slice progress.</div>
            </div>
          `;
          return;
        }

        const detail = await fetchJson(`/api/feature-progress?feature_id=${encodeURIComponent(featureId)}`);
        selectedFeatureId = featureId;
        featureDetailPanel.innerHTML = `
          <div class="feature-card">
            <div class="row">
              <strong>${escapeHtml(detail.feature.title)}</strong>
              ${badge(detail.status)}
            </div>
            <div class="muted">${escapeHtml(detail.feature.id)}</div>
            <div class="actions">
              <button type="button" id="detail-execute">Execute Next Slice</button>
              <button type="button" class="secondary" id="detail-refresh">Refresh Detail</button>
              ${detail.active_slice ? `<button type="button" class="secondary" id="detail-open-artifacts">Open Active Slice Artifacts</button>` : ""}
            </div>
          </div>
          <div class="stats">
            <div class="stat"><span class="label">Pending</span><strong>${detail.counts.pending}</strong></div>
            <div class="stat"><span class="label">In Progress</span><strong>${detail.counts.in_progress}</strong></div>
            <div class="stat"><span class="label">Completed</span><strong>${detail.counts.completed}</strong></div>
            <div class="stat"><span class="label">Attention</span><strong>${detail.counts.blocked_retry + detail.counts.escalated + detail.counts.refused}</strong></div>
          </div>
          <div class="timeline">
            ${detail.slices.map((slice) => `
              <div class="${timelineItemClass(slice)}" data-slice-id="${slice.id}">
                <div class="row">
                  <strong>${escapeHtml(slice.title)}</strong>
                  ${badge(slice.status)}
                </div>
                <div class="muted">${escapeHtml(slice.id)}</div>
                <div class="muted">${slice.depends_on.length > 0 ? `depends on ${escapeHtml(slice.depends_on.join(", "))}` : "no dependencies"}</div>
              </div>
            `).join("")}
          </div>
        `;

        document.getElementById("detail-execute").addEventListener("click", async () => {
          const payload = await postJson("/api/execute-feature", { feature_id: detail.feature.id });
          setLog(payload);
          await refreshDashboard();
        });

        document.getElementById("detail-refresh").addEventListener("click", async () => {
          await loadFeatureDetail(detail.feature.id);
        });

        const detailArtifacts = document.getElementById("detail-open-artifacts");
        if (detailArtifacts) {
          detailArtifacts.addEventListener("click", async () => {
            await loadSliceArtifacts(detail.active_slice.id);
          });
        }

        featureDetailPanel.querySelectorAll("[data-slice-id]").forEach((item) => {
          item.addEventListener("click", async () => {
            await loadSliceArtifacts(item.dataset.sliceId);
          });
        });
      }

      async function fetchJson(url, options) {
        const response = await fetch(url, options);
        const body = await response.json();
        if (!response.ok) {
          throw new Error(body.message || body.status || "request failed");
        }
        return body;
      }

      async function safeFetchJson(url, options) {
        try {
          return await fetchJson(url, options);
        } catch (error) {
          return { ok: false, status: "error", message: error.message };
        }
      }

      async function postJson(url, body) {
        return fetchJson(url, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body)
        });
      }

      async function runAction(action) {
        try {
          const payload = await action();
          setLog(payload);
          await refreshDashboard();
        } catch (error) {
          setLog({ ok: false, status: "error", message: error.message });
        }
      }

      async function refreshDashboard() {
        const [data, previewPlan] = await Promise.all([
          fetchJson("/api/dashboard"),
          safeFetchJson("/api/preview-plan")
        ]);
        const buildLog = dashboardState?.buildLog || await safeFetchJson("/api/build-log?limit=12");
        const previewLog = dashboardState?.previewLog || await safeFetchJson("/api/preview-log?limit=80");
        const queueStatus = dashboardState?.queueStatus || await safeFetchJson("/api/queue-status");
        const activityFeed = dashboardState?.activityFeed || await safeFetchJson("/api/activity-feed?limit=16");
        dashboardState = { dashboard: data, previewPlan, buildLog, previewLog, queueStatus, activityFeed };
        projectSummary.innerHTML = projectCard(data, previewPlan);
        renderBootstrapHealth(data);
        renderPreviewBuild(data, previewPlan);
        renderDebugDetail(dashboardState);
        renderQueueControl(dashboardState);
        renderActivityFeed(dashboardState);
        renderBacklog(data);
        renderActiveFeature(data);
        await loadFeatureDetail(selectedFeatureId || data.active_feature?.feature.id);
        if (dashboardState.sliceArtifacts?.slice_id) {
          await loadSliceArtifacts(dashboardState.sliceArtifacts.slice_id);
        } else {
          await loadSliceArtifacts(data.active_feature?.active_slice?.id);
        }
      }

      document.getElementById("refresh").addEventListener("click", refreshDashboard);
      document.getElementById("feature-form").addEventListener("submit", async (event) => {
        event.preventDefault();
        const payload = await postJson("/api/feature-flow", {
          title: document.getElementById("feature-title").value,
          description: document.getElementById("feature-description").value
        });
        setLog(payload);
        event.target.reset();
        await refreshDashboard();
      });

      setInterval(() => {
        refreshDashboard().catch((error) => {
          setLog({ ok: false, status: "error", message: error.message });
        });
      }, 10000);

      refreshDashboard()
        .then(() => {
          if (responseLog.textContent.trim() === "{}" && dashboardState) {
            setLog(dashboardState.dashboard);
          }
        })
        .catch((error) => {
          setLog({ ok: false, status: "error", message: error.message });
        });
    </script>
  </body>
</html>
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{DashboardServeOptions, dashboard_server_info, route_dashboard_request};
    use crate::adapter::HostKind;
    use crate::project::{
        ProjectAddFeatureOptions, ProjectCreateOptions, add_feature, create_project,
    };
    use std::fs;
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dashboard_info_reports_url() {
        let workspace = TestWorkspace::new("dashboard-info");
        let listener = TcpListener::bind("127.0.0.1:0").expect("port should bind");
        let port = listener.local_addr().expect("addr should resolve").port();
        drop(listener);

        let info = dashboard_server_info(&DashboardServeOptions {
            workspace_root: workspace.root.clone(),
            bind: "127.0.0.1".to_string(),
            port,
            host: HostKind::Stub,
        })
        .expect("server info should resolve");

        assert_eq!(info.status, "listening");
        assert_eq!(info.url, format!("http://127.0.0.1:{port}/"));
    }

    #[test]
    fn route_dashboard_root_serves_html() {
        let workspace = TestWorkspace::new("dashboard-root");
        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("root should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "text/html; charset=utf-8");
        assert!(body.contains("Mutagen Harness"));
        assert!(body.contains("/api/dashboard"));
    }

    #[test]
    fn route_dashboard_api_serves_snapshot() {
        let workspace = TestWorkspace::new("dashboard-api");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");
        add_feature(ProjectAddFeatureOptions {
            workspace_root: workspace.root.clone(),
            title: "Add due dates".to_string(),
            description: String::new(),
        })
        .expect("feature should be recorded");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/dashboard",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("dashboard should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"feature_backlog\""));
        assert!(body.contains("\"queued\": 1"));
    }

    #[test]
    fn route_dashboard_preview_plan_serves_configured_preview() {
        let workspace = TestWorkspace::new("dashboard-preview-plan");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/preview-plan",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("preview plan should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"command_kind\": \"dev\""));
        assert!(body.contains("http://localhost:5173"));
    }

    #[test]
    fn route_dashboard_run_command_supports_dry_run() {
        let workspace = TestWorkspace::new("dashboard-run-command");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/run-command",
            br#"{"kind":"build","dry_run":true}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("run command should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"dry_run\""));
        assert!(body.contains("\"command_kind\": \"build\""));
    }

    #[test]
    fn route_dashboard_build_log_returns_recent_entries() {
        let workspace = TestWorkspace::new("dashboard-build-log");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        fs::write(
            workspace.root.join(".mutagen/state/build-log.jsonl"),
            "{\"status\":\"completed\",\"command_kind\":\"test\"}\n{\"status\":\"failed\",\"command_kind\":\"build\"}\n",
        )
        .expect("build log should be written");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/build-log?limit=1",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("build log should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"entries\""));
        assert!(body.contains("\"command_kind\":\"build\""));
        assert!(!body.contains("\"command_kind\":\"test\""));
    }

    #[test]
    fn route_dashboard_preview_log_returns_tail_lines() {
        let workspace = TestWorkspace::new("dashboard-preview-log");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        fs::write(
            workspace.root.join(".mutagen/state/preview-output.log"),
            "line one\nline two\nline three\n",
        )
        .expect("preview log should be written");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/preview-log?limit=2",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("preview log should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("line two"));
        assert!(body.contains("line three"));
        assert!(!body.contains("line one"));
    }

    #[test]
    fn route_dashboard_slice_artifacts_returns_evidence_and_reviews() {
        let workspace = TestWorkspace::new("dashboard-slice-artifacts");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        fs::create_dir_all(workspace.root.join(".mutagen/state/evidence"))
            .expect("evidence dir should be created");
        fs::create_dir_all(workspace.root.join("reviews/slice-001"))
            .expect("review dir should be created");
        fs::write(
            workspace.root.join(".mutagen/state/evidence/slice-001.md"),
            "evidence body",
        )
        .expect("evidence bundle should be written");
        fs::write(
            workspace.root.join("reviews/slice-001/tiger-claw.md"),
            "qa report body",
        )
        .expect("review artifact should be written");
        fs::write(
            workspace.root.join(".mutagen/state/tiger-claw-latest.md"),
            "latest qa body",
        )
        .expect("latest qa report should be written");
        fs::write(
            workspace.root.join(".mutagen/state/active-slice.json"),
            r#"{
              "slice_id": "slice-001",
              "title": "Slice One",
              "evidence_bundle_path": ".mutagen/state/evidence/slice-001.md",
              "author_agent": "Bebop",
              "active_agent": "Bebop",
              "stage": "author",
              "pipeline_mode": "full",
              "review_required": true,
              "layer": 1,
              "bounded_context": "demo",
              "context_to_update": "project_state.md",
              "attempts": 0,
              "max_retries": 2,
              "micro_corrections_used": 0,
              "max_micro_corrections": 1,
              "allowed_write_globs": ["src/**"],
              "host": "stub",
              "degraded_capabilities": []
            }"#,
        )
        .expect("active state should be written");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/slice-artifacts?slice_id=slice-001",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("slice artifacts should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"slice_id\": \"slice-001\""));
        assert!(body.contains("evidence body"));
        assert!(body.contains("qa report body"));
        assert!(body.contains("latest qa body"));
        assert!(body.contains("\"active_state\""));
    }

    #[test]
    fn route_dashboard_mark_blocked_updates_queue_and_clears_active_state() {
        let workspace = TestWorkspace::new("dashboard-mark-blocked");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        fs::write(
            workspace.root.join("slices/queue.json"),
            r#"{
              "version": 1,
              "slices": [
                {
                  "id": "slice-001",
                  "title": "Slice One",
                  "status": "in_progress",
                  "author_agent": "Bebop",
                  "layer": 1,
                  "bounded_context": "demo",
                  "target_loc": 10,
                  "objective": "demo",
                  "context_to_update": "project_state.md",
                  "implementation_details": [],
                  "review_required": true,
                  "attempts": 0,
                  "micro_corrections_used": 0,
                  "depends_on": [],
                  "adjacent_scope_allowed": [],
                  "write_set": ["src/**"],
                  "traces_to": {},
                  "verification_steps": {},
                  "human_check_needed": {}
                }
              ]
            }"#,
        )
        .expect("queue should be written");

        fs::write(
            workspace.root.join(".mutagen/state/active-slice.json"),
            r#"{
              "slice_id": "slice-001",
              "title": "Slice One",
              "evidence_bundle_path": ".mutagen/state/evidence/slice-001.md",
              "author_agent": "Bebop",
              "active_agent": "Bebop",
              "stage": "author",
              "pipeline_mode": "full",
              "review_required": true,
              "layer": 1,
              "bounded_context": "demo",
              "context_to_update": "project_state.md",
              "attempts": 0,
              "max_retries": 2,
              "micro_corrections_used": 0,
              "max_micro_corrections": 1,
              "allowed_write_globs": ["src/**"],
              "host": "stub",
              "degraded_capabilities": []
            }"#,
        )
        .expect("active state should be written");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/slice-mark-blocked",
            br#"{"slice_id":"slice-001","reason":"Waiting on user input"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("slice mark blocked should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"operation\": \"mark_blocked\""));
        assert!(body.contains("\"active_state_cleared\": true"));
        assert!(
            !workspace
                .root
                .join(".mutagen/state/active-slice.json")
                .exists()
        );

        let queue = fs::read_to_string(workspace.root.join("slices/queue.json"))
            .expect("queue should still exist");
        assert!(queue.contains("\"status\": \"blocked_retry\""));
        assert!(queue.contains("Waiting on user input"));
    }

    #[test]
    fn route_dashboard_queue_status_reports_next_ready_slice() {
        let workspace = TestWorkspace::new("dashboard-queue-status");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        fs::write(
            workspace.root.join("slices/queue.json"),
            r#"{
              "version": 1,
              "slices": [
                {
                  "id": "slice-001",
                  "title": "Slice One",
                  "status": "pending",
                  "author_agent": "Bebop",
                  "layer": 1,
                  "bounded_context": "demo",
                  "target_loc": 10,
                  "objective": "demo",
                  "context_to_update": "project_state.md",
                  "implementation_details": ["demo"],
                  "review_required": true,
                  "attempts": 0,
                  "micro_corrections_used": 0,
                  "depends_on": [],
                  "adjacent_scope_allowed": [],
                  "write_set": ["src/**"],
                  "traces_to": { "prd": ["demo"] },
                  "verification_steps": { "acceptance": "demo" },
                  "human_check_needed": {}
                }
              ]
            }"#,
        )
        .expect("queue should be written");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/queue-status",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("queue status should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"ready\""));
        assert!(body.contains("\"next_ready_slice\""));
    }

    #[test]
    fn route_dashboard_resume_slice_sets_pending_and_clears_escalation() {
        let workspace = TestWorkspace::new("dashboard-resume-slice");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        fs::write(
            workspace.root.join("slices/queue.json"),
            r#"{
              "version": 1,
              "slices": [
                {
                  "id": "slice-001",
                  "title": "Slice One",
                  "status": "blocked_retry",
                  "author_agent": "Bebop",
                  "layer": 1,
                  "bounded_context": "demo",
                  "target_loc": 10,
                  "objective": "demo",
                  "context_to_update": "project_state.md",
                  "implementation_details": ["demo"],
                  "review_required": true,
                  "attempts": 1,
                  "micro_corrections_used": 0,
                  "depends_on": [],
                  "adjacent_scope_allowed": [],
                  "write_set": ["src/**"],
                  "traces_to": { "prd": ["demo"] },
                  "verification_steps": { "acceptance": "demo" },
                  "human_check_needed": {},
                  "escalation_reason": "stuck"
                }
              ]
            }"#,
        )
        .expect("queue should be written");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/slice-resume",
            br#"{"slice_id":"slice-001"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("slice resume should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"operation\": \"resume\""));

        let queue = fs::read_to_string(workspace.root.join("slices/queue.json"))
            .expect("queue should exist");
        assert!(queue.contains("\"status\": \"pending\""));
        assert!(!queue.contains("stuck"));
    }

    #[test]
    fn route_dashboard_activity_feed_merges_build_and_dispatch_events() {
        let workspace = TestWorkspace::new("dashboard-activity-feed");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        fs::write(
            workspace.root.join(".mutagen/state/build-log.jsonl"),
            "{\"event\":\"project_command\",\"command_kind\":\"build\",\"status\":\"failed\",\"recorded_at\":\"2026-04-25T16:00:00Z\"}\n",
        )
        .expect("build log should be written");
        fs::write(
            workspace.root.join(".mutagen/state/dispatch-log.jsonl"),
            "{\"slice_id\":\"slice-001\",\"status\":\"completed\",\"completed_at\":\"2026-04-25T16:01:00Z\"}\n",
        )
        .expect("dispatch log should be written");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/activity-feed?limit=5",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("activity feed should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"kind\": \"dispatch\""));
        assert!(body.contains("\"kind\": \"build\""));
        assert!(body.contains("slice-001 completed"));
        assert!(body.contains("build failed"));
    }

    #[test]
    fn route_dashboard_doctor_reports_tooling_status() {
        let workspace = TestWorkspace::new("dashboard-doctor");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/doctor",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("doctor should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"stack\": \"vite-express-sqlite\""));
        assert!(body.contains("\"checks\""));
    }

    #[test]
    fn route_dashboard_repair_scaffold_restores_missing_scaffold_file() {
        let workspace = TestWorkspace::new("dashboard-repair");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        fs::remove_file(workspace.root.join("src/App.jsx"))
            .expect("scaffold file should be removed");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/repair-scaffold",
            b"{}",
            &workspace.root,
            HostKind::Stub,
        )
        .expect("repair should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(
            body.contains("\"status\": \"repaired\"")
                || body.contains("\"status\": \"repaired_with_overwrites\"")
        );
        assert!(workspace.root.join("src/App.jsx").exists());
    }

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "mutagen-harness-{name}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("test workspace should be created");

            Self { root }
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
