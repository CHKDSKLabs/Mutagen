use crate::adapter::{HostKind, resolved_host_profile};
use crate::config::load_workflow_config_file;
use crate::finalize::{FinalizeSliceOptions, finalize_slice};
use crate::project::{
    ProjectCapsule, ProjectCommandKind, ProjectCreateOptions, ProjectDashboardOptions,
    ProjectDoctorOptions, ProjectExecuteFeatureOptions, ProjectFeatureFlowOptions,
    ProjectFeatureProgressOptions, ProjectInspectOptions, ProjectIntakeOptions,
    ProjectPreviewCheckOptions, ProjectPreviewLifecycleOptions, ProjectPreviewPlanOptions,
    ProjectRepairOptions, ProjectRunCommandOptions, ProjectVerifyGeneratedOptions, create_project,
    dashboard_project, doctor_project, execute_feature, feature_flow, feature_progress,
    inspect_project, list_blueprints, preview_check, preview_plan, preview_start, preview_status,
    preview_stop, project_intake, repair_project, run_project_command, verify_generated_project,
};
use crate::queue::SliceStatus;
use crate::queue_update::{UpdateSliceOptions, update_slice};
use crate::runtime::{PrepareNextOptions, prepare_next};
use crate::validation::{load_queue_file, validate_queue_file};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
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
struct ProjectIntakeRequest {
    prompt: String,
    #[serde(default)]
    queue_feature: bool,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
struct ProjectCreateRequest {
    name: String,
    stack: String,
    design_system: String,
    #[serde(default)]
    deploy_target: Option<String>,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
struct BuilderMessageRequest {
    message: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
struct HarnessChatRequest {
    message: String,
}

#[derive(Debug, Deserialize)]
struct TerminalCommandRequest {
    command: String,
}

#[derive(Debug, Deserialize)]
struct TerminalCancelRequest {
    id: String,
}

#[derive(Debug, Deserialize)]
struct DesignDocRequest {
    document: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct DesignDocSeedRequest {
    document: String,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
struct DesignBundleSeedRequest {
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
struct ExecutionRunRequest {
    #[serde(default)]
    host: Option<HostKind>,
    #[serde(default)]
    dry_run: bool,
}

#[derive(Debug, Deserialize)]
struct ExecutionCancelRequest {
    id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExecutionJob {
    id: String,
    ok: bool,
    status: String,
    workspace_root: String,
    host: HostKind,
    dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    command: Vec<String>,
    started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ended_at: Option<String>,
    updated_at: String,
    stdout_path: String,
    stderr_path: String,
    metadata_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(default)]
    completed_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    terminal: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TerminalJob {
    id: String,
    ok: bool,
    status: String,
    workspace_root: String,
    cwd: String,
    shell: String,
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pid: Option<u32>,
    started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ended_at: Option<String>,
    updated_at: String,
    stdout_path: String,
    stderr_path: String,
    metadata_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
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

#[derive(Debug, Deserialize)]
struct InferenceHostRequest {
    host: HostKind,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct DashboardInferenceSettings {
    host: HostKind,
    updated_at: String,
}

pub fn dashboard_server_info(options: &DashboardServeOptions) -> Result<DashboardServeResult> {
    fs::create_dir_all(&options.workspace_root).with_context(|| {
        format!(
            "failed to create workspace root at {}",
            options.workspace_root.display()
        )
    })?;

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
            let body = if workspace_root.join(".mutagen/project.json").exists() {
                serde_json::to_value(dashboard_project(ProjectDashboardOptions {
                    workspace_root: workspace_root.to_path_buf(),
                })?)?
            } else {
                uninitialized_dashboard(workspace_root)
            };
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/project-blueprints") => Ok((
            200,
            "application/json".to_string(),
            serde_json::to_string_pretty(&list_blueprints())?,
        )),
        (&Method::Get, "/api/inference-host") => {
            let body = inference_host_state(workspace_root, default_host)?;
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
        (&Method::Get, "/api/builder-thread") => {
            let limit = query_limit(&query).unwrap_or(40);
            let body = builder_thread(workspace_root, limit)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/harness-chat") => {
            let limit = query_limit(&query).unwrap_or(80);
            let body = harness_chat_history(workspace_root, limit)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/harness-terminal") => {
            let limit = query_limit(&query).unwrap_or(20);
            let body = terminal_jobs(workspace_root, limit, 120)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/design-bundle") => {
            let body = design_bundle(workspace_root)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/build-readiness") => {
            let body = build_readiness(workspace_root)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/execution-jobs") => {
            let limit = query_limit(&query).unwrap_or(20);
            let body = execution_jobs(workspace_root, limit)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Get, "/api/execution-job") => {
            let id = query_value(&query, "id")
                .filter(|value| !value.is_empty())
                .ok_or_else(|| anyhow::anyhow!("`id` query parameter is required"))?;
            let body = execution_job_detail(workspace_root, &id, 160)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
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
            let host = payload.host.unwrap_or(resolve_dashboard_inference_host(
                workspace_root,
                default_host,
            )?);
            let body = prepare_next(PrepareNextOptions {
                workspace_root: workspace_root.to_path_buf(),
                queue_path: workspace_root.join("slices/queue.json"),
                workflow_config_path: workspace_root.join(".claude/workflow.json"),
                active_state_path: workspace_root.join(".mutagen/state/active-slice.json"),
                host,
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
        (&Method::Post, "/api/project-intake") => {
            let payload: ProjectIntakeRequest = read_json_body(body)?;
            let body = project_intake(ProjectIntakeOptions {
                workspace_root: workspace_root.to_path_buf(),
                prompt: payload.prompt,
                queue_feature: payload.queue_feature,
                force: payload.force,
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/project-create") => {
            let payload: ProjectCreateRequest = read_json_body(body)?;
            let body = create_project(ProjectCreateOptions {
                workspace_root: workspace_root.to_path_buf(),
                name: payload.name,
                stack: payload.stack,
                design_system: payload.design_system,
                deploy_target: payload.deploy_target,
                force: payload.force,
            })?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/builder-message") => {
            let payload: BuilderMessageRequest = read_json_body(body)?;
            let body = record_builder_message(workspace_root, payload)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/harness-chat") => {
            let payload: HarnessChatRequest = read_json_body(body)?;
            let body = handle_harness_chat(workspace_root, default_host, payload)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/harness-terminal") => {
            let payload: TerminalCommandRequest = read_json_body(body)?;
            let body = start_terminal_job(workspace_root, payload)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/harness-terminal-cancel") => {
            let payload: TerminalCancelRequest = read_json_body(body)?;
            let body = cancel_terminal_job(workspace_root, &payload.id)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/design-doc") => {
            let payload: DesignDocRequest = read_json_body(body)?;
            let body = write_design_doc(workspace_root, payload)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/design-doc-seed") => {
            let payload: DesignDocSeedRequest = read_json_body(body)?;
            let body = seed_design_doc(workspace_root, payload)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/design-bundle-seed") => {
            let payload: DesignBundleSeedRequest = if body.is_empty() {
                DesignBundleSeedRequest { force: false }
            } else {
                read_json_body(body)?
            };
            let body = seed_design_bundle(workspace_root, payload)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/execution-run") => {
            let payload: ExecutionRunRequest = if body.is_empty() {
                ExecutionRunRequest {
                    host: None,
                    dry_run: false,
                }
            } else {
                read_json_body(body)?
            };
            let host = payload.host.unwrap_or(resolve_dashboard_inference_host(
                workspace_root,
                default_host,
            )?);
            let body = start_execution_job(workspace_root, host, payload.dry_run)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/execution-cancel") => {
            let payload: ExecutionCancelRequest = read_json_body(body)?;
            let body = cancel_execution_job(workspace_root, &payload.id)?;
            Ok((
                200,
                "application/json".to_string(),
                serde_json::to_string_pretty(&body)?,
            ))
        }
        (&Method::Post, "/api/inference-host") => {
            let payload: InferenceHostRequest = read_json_body(body)?;
            let body = set_inference_host(workspace_root, default_host, payload.host)?;
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
            let host = payload.host.unwrap_or(resolve_dashboard_inference_host(
                workspace_root,
                default_host,
            )?);
            let body = execute_feature(ProjectExecuteFeatureOptions {
                workspace_root: workspace_root.to_path_buf(),
                feature_id: payload.feature_id,
                host,
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

fn uninitialized_dashboard(workspace_root: &Path) -> serde_json::Value {
    let capsule_path = workspace_root.join(".mutagen/project.json");

    json!({
        "ok": false,
        "status": "uninitialized",
        "workspace_root": workspace_root.to_string_lossy(),
        "capsule_path": capsule_path.to_string_lossy(),
        "message": "Create a project capsule to start using this workspace.",
    })
}

fn builder_thread_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".mutagen/state/builder-thread.jsonl")
}

fn builder_thread(workspace_root: &Path, limit: usize) -> Result<serde_json::Value> {
    let path = builder_thread_path(workspace_root);
    let messages = read_jsonl_tail(&path, limit)?;

    Ok(json!({
        "ok": true,
        "status": "ready",
        "path": path.to_string_lossy(),
        "messages": messages,
    }))
}

fn record_builder_message(
    workspace_root: &Path,
    request: BuilderMessageRequest,
) -> Result<serde_json::Value> {
    let message = request.message.trim();
    if message.is_empty() {
        bail!("builder message is required");
    }

    let action = normalize_builder_action(&request.action)?;
    let path = builder_thread_path(workspace_root);
    let user_message = json!({
        "role": "user",
        "action": action,
        "content": message,
        "created_at": now_rfc3339()?,
    });
    append_jsonl(&path, &user_message)?;

    let (ok, status, assistant_content, artifact) = match action {
        "note" => (
            true,
            "recorded".to_string(),
            "Noted. I kept that in the builder thread.".to_string(),
            serde_json::Value::Null,
        ),
        "save_direction" => match project_intake(ProjectIntakeOptions {
            workspace_root: workspace_root.to_path_buf(),
            prompt: message.to_string(),
            queue_feature: false,
            force: request.force,
        }) {
            Ok(result) => {
                let title = result.title.clone();
                let status = result.status.clone();
                (
                    result.ok,
                    status,
                    format!("Saved that to the design brief as `{title}`."),
                    serde_json::to_value(result)?,
                )
            }
            Err(error) => (
                false,
                "action_failed".to_string(),
                format!("I kept the message, but could not update the design brief: {error}"),
                json!({
                    "ok": false,
                    "status": "action_failed",
                    "message": error.to_string(),
                }),
            ),
        },
        "queue_work" => match project_intake(ProjectIntakeOptions {
            workspace_root: workspace_root.to_path_buf(),
            prompt: message.to_string(),
            queue_feature: true,
            force: request.force,
        }) {
            Ok(result) => {
                let title = result.title.clone();
                let status = result.status.clone();
                let content = if result.ok {
                    format!("Saved that direction and queued `{title}` as executable work.")
                } else {
                    format!(
                        "Saved that direction, but queueing `{title}` hit a problem. The details are in the action result."
                    )
                };
                (result.ok, status, content, serde_json::to_value(result)?)
            }
            Err(error) => (
                false,
                "action_failed".to_string(),
                format!("I kept the message, but could not queue work yet: {error}"),
                json!({
                    "ok": false,
                    "status": "action_failed",
                    "message": error.to_string(),
                }),
            ),
        },
        _ => unreachable!("builder action was normalized"),
    };

    let assistant_message = json!({
        "role": "assistant",
        "action": action,
        "content": assistant_content,
        "created_at": now_rfc3339()?,
        "artifact": artifact,
    });
    append_jsonl(&path, &assistant_message)?;

    Ok(json!({
        "ok": ok,
        "status": status,
        "path": path.to_string_lossy(),
        "user_message": user_message,
        "assistant_message": assistant_message,
        "messages": read_jsonl_tail(&path, 40)?,
    }))
}

fn normalize_builder_action(action: &str) -> Result<&'static str> {
    match action.trim() {
        "" | "note" => Ok("note"),
        "save_direction" => Ok("save_direction"),
        "queue_work" => Ok("queue_work"),
        other => bail!(
            "unsupported builder action `{}`; expected note, save_direction, or queue_work",
            other
        ),
    }
}

fn harness_chat_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".mutagen/state/dashboard-chat.jsonl")
}

fn harness_chat_history(workspace_root: &Path, limit: usize) -> Result<serde_json::Value> {
    let path = harness_chat_path(workspace_root);
    let messages = read_jsonl_tail(&path, limit)?;

    Ok(json!({
        "ok": true,
        "status": "ready",
        "path": path.to_string_lossy(),
        "messages": messages,
    }))
}

fn handle_harness_chat(
    workspace_root: &Path,
    default_host: HostKind,
    request: HarnessChatRequest,
) -> Result<serde_json::Value> {
    let message = request.message.trim();
    if message.is_empty() {
        bail!("chat message is required");
    }

    let path = harness_chat_path(workspace_root);
    let user_message = json!({
        "role": "user",
        "content": message,
        "created_at": now_rfc3339()?,
    });
    append_jsonl(&path, &user_message)?;

    let (ok, status, content, actions) =
        match run_harness_chat_intent(workspace_root, default_host, message) {
            Ok(result) => result,
            Err(error) => (
                false,
                "action_failed".to_string(),
                format!("I tried to run that through the harness, but it failed: {error}"),
                vec![chat_action(
                    "error",
                    json!({
                        "ok": false,
                        "status": "action_failed",
                        "message": error.to_string(),
                    }),
                )],
            ),
        };
    let assistant_message = json!({
        "role": "assistant",
        "content": content,
        "created_at": now_rfc3339()?,
        "status": status,
        "actions": actions,
    });
    append_jsonl(&path, &assistant_message)?;

    Ok(json!({
        "ok": ok,
        "status": status,
        "path": path.to_string_lossy(),
        "user_message": user_message,
        "assistant_message": assistant_message,
        "messages": read_jsonl_tail(&path, 80)?,
    }))
}

fn run_harness_chat_intent(
    workspace_root: &Path,
    default_host: HostKind,
    message: &str,
) -> Result<(bool, String, String, Vec<serde_json::Value>)> {
    let normalized = message.trim();
    let lower = normalized.to_ascii_lowercase();
    let command = lower.strip_prefix('/').unwrap_or(&lower);

    if command == "help" || lower.contains("what can you do") {
        return Ok((
            true,
            "help".to_string(),
            harness_chat_help_text(),
            Vec::new(),
        ));
    }

    if command == "blueprints" || command == "stacks" {
        let result = serde_json::to_value(list_blueprints())?;
        return Ok((
            true,
            "blueprints".to_string(),
            "Here are the stacks the harness can create and scaffold.".to_string(),
            vec![chat_action("blueprints", result)],
        ));
    }

    if command.starts_with("create ") || lower.starts_with("create project ") {
        let input = normalized
            .strip_prefix("/create")
            .or_else(|| normalized.strip_prefix("create project"))
            .or_else(|| normalized.strip_prefix("create"))
            .unwrap_or("")
            .trim();
        let args = parse_chat_named_args(input);
        let name = chat_arg(&args, "name")
            .or_else(|| chat_arg(&args, "project"))
            .map(str::to_string)
            .or_else(|| infer_chat_project_name(input))
            .unwrap_or_else(|| "Mutagen Project".to_string());
        let stack = chat_arg(&args, "stack")
            .map(str::to_string)
            .or_else(|| infer_chat_stack(input))
            .unwrap_or_else(|| "vite-express-sqlite".to_string());
        let design_system = chat_arg(&args, "design")
            .or_else(|| chat_arg(&args, "design_system"))
            .unwrap_or("plain-css")
            .to_string();
        let deploy_target = chat_arg(&args, "deploy")
            .or_else(|| chat_arg(&args, "deploy_target"))
            .map(str::to_string);
        let force = chat_arg(&args, "force")
            .map(|value| matches!(value, "true" | "yes" | "1"))
            .unwrap_or(false);

        let result = create_project(ProjectCreateOptions {
            workspace_root: workspace_root.to_path_buf(),
            name: name.clone(),
            stack: stack.clone(),
            design_system,
            deploy_target,
            force,
        })?;
        let ok = result.ok;
        let status = result.status.clone();
        return Ok((
            ok,
            status,
            format!("Created `{name}` on the `{stack}` stack."),
            vec![chat_action("project-create", serde_json::to_value(result)?)],
        ));
    }

    if command == "status"
        || command == "readiness"
        || lower.contains("status")
        || lower.contains("build readiness")
        || lower.contains("what is blocked")
    {
        let readiness = build_readiness(workspace_root)?;
        return Ok((
            readiness
                .get("ok")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            readiness
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("ready")
                .to_string(),
            summarize_readiness(&readiness),
            vec![chat_action("build-readiness", readiness)],
        ));
    }

    if command == "snapshot" || command == "dashboard" {
        let snapshot = if workspace_root.join(".mutagen/project.json").exists() {
            serde_json::to_value(dashboard_project(ProjectDashboardOptions {
                workspace_root: workspace_root.to_path_buf(),
            })?)?
        } else {
            uninitialized_dashboard(workspace_root)
        };
        return Ok((
            snapshot
                .get("ok")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            snapshot
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("ready")
                .to_string(),
            "Here is the current dashboard snapshot.".to_string(),
            vec![chat_action("dashboard", snapshot)],
        ));
    }

    if command == "seed-design" || command == "seed design" || lower.contains("seed design") {
        let result = seed_design_bundle(workspace_root, DesignBundleSeedRequest { force: false })?;
        return Ok((
            true,
            "design_seeded".to_string(),
            "Seeded every missing or draft design document. The design bundle should now be ready unless something truly weird happened.".to_string(),
            vec![chat_action("design-bundle-seed", result)],
        ));
    }

    if command == "repair-safe" || command == "safe repairs" || lower.contains("run safe repairs") {
        let actions = run_harness_chat_safe_repairs(workspace_root)?;
        return Ok((
            true,
            "safe_repairs_complete".to_string(),
            "Ran the safe repair pass. I did not queue work or start the execution loop."
                .to_string(),
            actions,
        ));
    }

    if command == "doctor" || command == "run doctor" {
        let result = doctor_project(ProjectDoctorOptions {
            workspace_root: workspace_root.to_path_buf(),
        })?;
        return Ok((
            result.ok,
            result.status.clone(),
            "Ran doctor checks for the current stack.".to_string(),
            vec![chat_action("doctor", serde_json::to_value(result)?)],
        ));
    }

    if let Some(kind) = chat_command_kind(command) {
        let result = run_project_command(ProjectRunCommandOptions {
            workspace_root: workspace_root.to_path_buf(),
            kind,
            dry_run: command.contains("dry"),
        })?;
        return Ok((
            result.ok,
            result.status.clone(),
            format!(
                "Ran `{}` through the harness command runner.",
                command_kind_name_for_chat(kind)
            ),
            vec![chat_action("run-command", serde_json::to_value(result)?)],
        ));
    }

    if command == "verify" || command == "verify-generated" || lower.contains("verify generated") {
        let result = verify_generated_project(ProjectVerifyGeneratedOptions {
            workspace_root: workspace_root.to_path_buf(),
        })?;
        return Ok((
            result.ok,
            result.status.clone(),
            "Ran the generated-project verification loop.".to_string(),
            vec![chat_action(
                "verify-generated",
                serde_json::to_value(result)?,
            )],
        ));
    }

    if command == "preview-start" || command == "start preview" || lower.contains("start preview") {
        let result = preview_start(ProjectPreviewLifecycleOptions {
            workspace_root: workspace_root.to_path_buf(),
        })?;
        return Ok((
            result.ok,
            result.status.clone(),
            "Started the configured preview command.".to_string(),
            vec![chat_action("preview-start", serde_json::to_value(result)?)],
        ));
    }

    if command == "preview-check" || command == "check preview" || lower.contains("check preview") {
        let result = preview_check(ProjectPreviewCheckOptions {
            workspace_root: workspace_root.to_path_buf(),
        })?;
        return Ok((
            result.ok,
            result.status.clone(),
            "Checked preview reachability.".to_string(),
            vec![chat_action("preview-check", serde_json::to_value(result)?)],
        ));
    }

    if command == "preview-stop" || command == "stop preview" || lower.contains("stop preview") {
        let result = preview_stop(ProjectPreviewLifecycleOptions {
            workspace_root: workspace_root.to_path_buf(),
        })?;
        return Ok((
            result.ok,
            result.status.clone(),
            "Stopped the managed preview process if one was running.".to_string(),
            vec![chat_action("preview-stop", serde_json::to_value(result)?)],
        ));
    }

    if command.starts_with("host ") || command.starts_with("set host ") {
        let host_value = command
            .strip_prefix("set host ")
            .or_else(|| command.strip_prefix("host "))
            .unwrap_or("")
            .trim()
            .trim_start_matches("to ")
            .trim();
        let host = parse_chat_host(host_value)?;
        let result = set_inference_host(workspace_root, default_host, host)?;
        return Ok((
            true,
            "host_updated".to_string(),
            format!("Set the dashboard inference host to `{}`.", host_name(host)),
            vec![chat_action("inference-host", result)],
        ));
    }

    if command == "jobs" || command == "runs" || lower.contains("execution jobs") {
        let result = execution_jobs(workspace_root, 12)?;
        return Ok((
            true,
            "execution_jobs".to_string(),
            "Here are the recent harness loop runs.".to_string(),
            vec![chat_action("execution-jobs", result)],
        ));
    }

    if command == "run"
        || command == "run dry"
        || lower.contains("run harness")
        || lower.contains("run the harness")
        || lower.contains("start harness")
        || lower.contains("start the harness")
    {
        let host = resolve_dashboard_inference_host(workspace_root, default_host)?;
        let dry_run = command.contains("dry") || lower.contains("dry run");
        let result = start_execution_job(workspace_root, host, dry_run)?;
        return Ok((
            result
                .get("ok")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            result
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("started")
                .to_string(),
            if dry_run {
                "Recorded a dry-run harness loop job without launching the runner.".to_string()
            } else {
                "Started the harness loop with the selected inference host.".to_string()
            },
            vec![chat_action("execution-run", result)],
        ));
    }

    if command.starts_with("cancel") {
        let requested_id = command.strip_prefix("cancel").unwrap_or("").trim();
        let job_id = if requested_id.is_empty()
            || requested_id == "current"
            || requested_id == "run"
        {
            running_execution_job(workspace_root)?
                .map(|job| job.id)
                .ok_or_else(|| anyhow::anyhow!("no running execution job is available to cancel"))?
        } else {
            requested_id.to_string()
        };
        let result = cancel_execution_job(workspace_root, &job_id)?;
        return Ok((
            result
                .get("ok")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            result
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("cancelled")
                .to_string(),
            format!("Sent a cancel request for `{job_id}`."),
            vec![chat_action("execution-cancel", result)],
        ));
    }

    if command.starts_with("queue ") || lower.starts_with("queue this ") {
        let prompt = normalized
            .strip_prefix("/queue")
            .or_else(|| normalized.strip_prefix("queue this"))
            .or_else(|| normalized.strip_prefix("queue"))
            .unwrap_or(normalized)
            .trim();
        let result = record_builder_message(
            workspace_root,
            BuilderMessageRequest {
                message: prompt.to_string(),
                action: "queue_work".to_string(),
                force: false,
            },
        )?;
        return Ok((
            result
                .get("ok")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            result
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("queued")
                .to_string(),
            "Sure. I saved that as project direction and queued it as executable work.".to_string(),
            vec![chat_action("builder-message", result)],
        ));
    }

    if command.starts_with("save ") || command.starts_with("direction ") {
        let prompt = normalized
            .strip_prefix("/save")
            .or_else(|| normalized.strip_prefix("save"))
            .or_else(|| normalized.strip_prefix("direction"))
            .unwrap_or(normalized)
            .trim();
        let result = record_builder_message(
            workspace_root,
            BuilderMessageRequest {
                message: prompt.to_string(),
                action: "save_direction".to_string(),
                force: false,
            },
        )?;
        return Ok((
            result
                .get("ok")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            result
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("saved")
                .to_string(),
            "Saved that into the project direction.".to_string(),
            vec![chat_action("builder-message", result)],
        ));
    }

    let result = record_builder_message(
        workspace_root,
        BuilderMessageRequest {
            message: normalized.to_string(),
            action: "note".to_string(),
            force: false,
        },
    )?;

    Ok((
        true,
        "noted".to_string(),
        "I saved that as context. Use `/queue your request` when you want me to turn it into harness work, or `/help` to see the commands I can run from here.".to_string(),
        vec![chat_action("builder-message", result)],
    ))
}

fn harness_chat_help_text() -> String {
    [
        "I can run the harness from this chat.",
        "Try `/status`, `/blueprints`, `/create name=\"Crew Scheduler\" stack=vite-express-sqlite design=plain-css`, `/seed-design`, `/repair-safe`, `/setup`, `/test`, `/build`, `/verify`, `/host codex`, `/queue add login`, `/run`, `/run dry`, `/jobs`, or `/cancel current`.",
        "Plain messages are saved as builder context. Nothing queues or executes unless you say so.",
    ]
    .join(" ")
}

fn chat_action(name: &str, result: serde_json::Value) -> serde_json::Value {
    let summary = summarize_chat_action(name, &result);

    json!({
        "name": name,
        "summary": summary,
        "result": result,
    })
}

fn summarize_chat_action(name: &str, result: &serde_json::Value) -> serde_json::Value {
    match name {
        "builder-message" => summarize_builder_message_action(result),
        "build-readiness" => summarize_build_readiness_action(result),
        "design-bundle-seed" => summarize_design_bundle_seed_action(result),
        "project-create" => summarize_project_create_action(result),
        "blueprints" => summarize_blueprints_action(result),
        "run-command" => summarize_run_command_action(result),
        "verify-generated" => summarize_verify_generated_action(result),
        "doctor" => summarize_doctor_action(result),
        "preview-start" | "preview-check" | "preview-stop" => {
            summarize_preview_action(name, result)
        }
        "execution-run" => summarize_execution_run_action(result),
        "execution-jobs" => summarize_execution_jobs_action(result),
        "execution-cancel" => summarize_execution_cancel_action(result),
        "terminal-command" => summarize_terminal_command_action(result),
        "inference-host" => summarize_inference_host_action(result),
        "error" => chat_summary(
            "Harness error",
            json_str(result, &["status"]).unwrap_or("action_failed"),
            vec![
                json_str(result, &["message"])
                    .unwrap_or("The harness returned an error.")
                    .to_string(),
            ],
        ),
        _ => summarize_generic_action(name, result),
    }
}

fn summarize_builder_message_action(result: &serde_json::Value) -> serde_json::Value {
    let mut lines = Vec::new();
    if let Some(content) = json_str(result, &["assistant_message", "content"]) {
        lines.push(content.to_string());
    }
    if let Some(title) = json_str(result, &["assistant_message", "artifact", "title"]) {
        lines.push(format!("Request title: {title}."));
    }
    if let Some(feature_id) = json_str(
        result,
        &[
            "assistant_message",
            "artifact",
            "feature_flow",
            "feature_id",
        ],
    ) {
        lines.push(format!("Feature id: {feature_id}."));
    }

    let enqueued = json_array_len(
        result,
        &[
            "assistant_message",
            "artifact",
            "feature_flow",
            "enqueue_feature",
            "enqueued_slice_ids",
        ],
    );
    if enqueued > 0 {
        lines.push(format!(
            "Prepared and queued {enqueued} implementation slice(s)."
        ));
    }

    let queue_count = json_usize(
        result,
        &[
            "assistant_message",
            "artifact",
            "feature_flow",
            "enqueue_feature",
            "queue_slice_count",
        ],
    );
    if queue_count > 0 {
        lines.push(format!("The queue now contains {queue_count} slice(s)."));
    }

    if let Some(error) = json_str(result, &["assistant_message", "artifact", "queue_error"]) {
        lines.push(format!("Queueing needs attention: {error}"));
    }

    if lines.is_empty() {
        lines.push("Recorded the message in the builder thread.".to_string());
    }

    chat_summary(
        "Builder request",
        json_str(result, &["status"]).unwrap_or("recorded"),
        lines,
    )
}

fn summarize_build_readiness_action(result: &serde_json::Value) -> serde_json::Value {
    let status = json_str(result, &["status"]).unwrap_or("unknown");
    let blockers = json_usize(result, &["blockers"]);
    let warnings = json_usize(result, &["warnings"]);
    let passed = json_usize(result, &["passed"]);
    let mut lines = vec![
        format!("Readiness is {status}."),
        format!("{passed} check(s) passing, {blockers} blocker(s), {warnings} warning(s)."),
    ];

    if json_bool(result, &["can_execute"]) {
        lines.push("The harness loop can run.".to_string());
    } else if let Some(check) = first_failed_blocker(result) {
        let label = json_str(check, &["label"]).unwrap_or("Next blocker");
        let detail = json_str(check, &["detail"]).unwrap_or("No detail was reported.");
        lines.push(format!("Next blocker: {label}. {detail}"));
    }

    chat_summary("Build readiness", status, lines)
}

fn summarize_design_bundle_seed_action(result: &serde_json::Value) -> serde_json::Value {
    let seeded = json_array_len(result, &["seeded"]);
    let skipped = json_array_len(result, &["skipped"]);
    let ready = json_usize(result, &["bundle", "readiness", "ready"]);
    let total = json_usize(result, &["bundle", "readiness", "total"]);
    chat_summary(
        "Design bundle",
        json_str(result, &["status"]).unwrap_or("seeded"),
        vec![
            format!("Seeded {seeded} document(s); skipped {skipped} already-ready document(s)."),
            format!("{ready}/{total} design document(s) are ready."),
        ],
    )
}

fn summarize_project_create_action(result: &serde_json::Value) -> serde_json::Value {
    let name = json_str(result, &["init", "capsule", "name"]).unwrap_or("project");
    let stack = json_str(result, &["blueprint", "blueprint", "stack"]).unwrap_or("unknown stack");
    let created = json_array_len(result, &["scaffold", "created_paths"]);
    chat_summary(
        "Project created",
        json_str(result, &["status"]).unwrap_or("created"),
        vec![
            format!("Created {name} using {stack}."),
            format!("Materialized {created} scaffold path(s)."),
        ],
    )
}

fn summarize_blueprints_action(result: &serde_json::Value) -> serde_json::Value {
    let blueprints = result
        .get("blueprints")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let names = blueprints
        .iter()
        .take(6)
        .filter_map(|blueprint| json_str(blueprint, &["stack"]))
        .collect::<Vec<_>>()
        .join(", ");
    chat_summary(
        "Blueprint catalog",
        json_str(result, &["status"]).unwrap_or("ready"),
        vec![format!(
            "Found {} stack blueprint(s): {names}.",
            blueprints.len()
        )],
    )
}

fn summarize_run_command_action(result: &serde_json::Value) -> serde_json::Value {
    let kind = json_str(result, &["command_kind"]).unwrap_or("command");
    let status = json_str(result, &["status"]).unwrap_or("unknown");
    let mut lines = vec![format!("Ran {kind}: {status}.")];

    if let Some(exit_code) = result.get("exit_code").and_then(|value| value.as_i64()) {
        lines.push(format!("Exit code: {exit_code}."));
    }
    push_first_output_line(&mut lines, "stdout", json_str(result, &["stdout"]));
    push_first_output_line(&mut lines, "stderr", json_str(result, &["stderr"]));
    if let Some(path) = json_str(result, &["build_log_path"]) {
        lines.push(format!("Logged under {path}."));
    }

    chat_summary("Project command", status, lines)
}

fn summarize_verify_generated_action(result: &serde_json::Value) -> serde_json::Value {
    let status = json_str(result, &["status"]).unwrap_or("unknown");
    let steps = result
        .get("steps")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut lines = vec![format!("Verification finished with status {status}.")];
    for step in steps.iter().take(5) {
        let name = json_str(step, &["name"]).unwrap_or("step");
        let step_status = json_str(step, &["status"]).unwrap_or("unknown");
        let detail = json_str(step, &["detail"]).unwrap_or("");
        lines.push(format!("{name}: {step_status}. {detail}"));
    }

    chat_summary("Generated project verification", status, lines)
}

fn summarize_doctor_action(result: &serde_json::Value) -> serde_json::Value {
    let status = json_str(result, &["status"]).unwrap_or("unknown");
    let mut lines = vec![format!("Doctor status: {status}.")];
    if let Some(checks) = result.get("checks").and_then(|value| value.as_array()) {
        for check in checks.iter().take(5) {
            let executable = json_str(check, &["executable"]).unwrap_or("tool");
            let check_status = json_str(check, &["status"]).unwrap_or("unknown");
            let detail = json_str(check, &["detail"]).unwrap_or("");
            lines.push(format!("{executable}: {check_status}. {detail}"));
        }
    }

    chat_summary("Tooling doctor", status, lines)
}

fn summarize_preview_action(name: &str, result: &serde_json::Value) -> serde_json::Value {
    let status = json_str(result, &["status"]).unwrap_or("unknown");
    let title = match name {
        "preview-start" => "Preview start",
        "preview-check" => "Preview check",
        "preview-stop" => "Preview stop",
        _ => "Preview",
    };
    let mut lines = vec![format!("Preview status: {status}.")];
    if let Some(detail) = json_str(result, &["detail"]) {
        lines.push(detail.to_string());
    }
    if let Some(url) = json_str(result, &["url"]) {
        lines.push(format!("Target: {url}."));
    }
    if let Some(command) = json_str(result, &["command"]) {
        if !command.trim().is_empty() {
            lines.push(format!("Command: {command}."));
        }
    }

    chat_summary(title, status, lines)
}

fn summarize_execution_run_action(result: &serde_json::Value) -> serde_json::Value {
    let status = json_str(result, &["status"]).unwrap_or("started");
    let mut lines = vec![format!("Execution job status: {status}.")];
    if let Some(id) = json_str(result, &["job", "id"]).or_else(|| json_str(result, &["id"])) {
        lines.push(format!("Job id: {id}."));
    }
    if let Some(host) = json_str(result, &["job", "host"]).or_else(|| json_str(result, &["host"])) {
        lines.push(format!("Host: {host}."));
    }
    if json_bool(result, &["job", "dry_run"]) || json_bool(result, &["dry_run"]) {
        lines.push("Dry run only; no runner process was launched.".to_string());
    } else {
        lines.push("The runner is now working in the background.".to_string());
    }

    chat_summary("Harness loop", status, lines)
}

fn summarize_execution_jobs_action(result: &serde_json::Value) -> serde_json::Value {
    let jobs = json_array_len(result, &["jobs"]);
    let mut lines = vec![format!("Found {jobs} recent harness loop job(s).")];
    if let Some(id) = json_str(result, &["current", "id"]) {
        let status = json_str(result, &["current", "status"]).unwrap_or("running");
        lines.push(format!("Current job: {id} ({status})."));
    }

    chat_summary(
        "Harness loop jobs",
        json_str(result, &["status"]).unwrap_or("ready"),
        lines,
    )
}

fn summarize_execution_cancel_action(result: &serde_json::Value) -> serde_json::Value {
    let status = json_str(result, &["status"]).unwrap_or("cancelled");
    let mut lines = vec![format!("Cancel request status: {status}.")];
    if let Some(id) = json_str(result, &["job", "id"]).or_else(|| json_str(result, &["id"])) {
        lines.push(format!("Job id: {id}."));
    }

    chat_summary("Harness loop cancel", status, lines)
}

fn summarize_terminal_command_action(result: &serde_json::Value) -> serde_json::Value {
    let status = json_str(result, &["status"]).unwrap_or("unknown");
    let command = json_str(result, &["command"]).unwrap_or("command");
    let mut lines = vec![format!("`{command}` is {status}.")];

    if let Some(id) = json_str(result, &["id"]) {
        lines.push(format!("Job id: {id}."));
    }
    if let Some(exit_code) = result.get("exit_code").and_then(|value| value.as_i64()) {
        lines.push(format!("Exit code: {exit_code}."));
    }
    if let Some(stdout) = result
        .get("stdout_lines")
        .and_then(|value| value.as_array())
    {
        for line in stdout.iter().filter_map(|value| value.as_str()).take(4) {
            lines.push(format!("stdout: {}", truncate_normalized_text(line, 180)));
        }
    }
    if let Some(stderr) = result
        .get("stderr_lines")
        .and_then(|value| value.as_array())
    {
        for line in stderr.iter().filter_map(|value| value.as_str()).take(4) {
            lines.push(format!("stderr: {}", truncate_normalized_text(line, 180)));
        }
    }

    chat_summary("Terminal command", status, lines)
}

fn summarize_inference_host_action(result: &serde_json::Value) -> serde_json::Value {
    let host = json_str(result, &["selected_host"]).unwrap_or("unknown");
    chat_summary(
        "Inference host",
        json_str(result, &["status"]).unwrap_or("host_updated"),
        vec![format!("Dashboard inference host is now {host}.")],
    )
}

fn summarize_generic_action(name: &str, result: &serde_json::Value) -> serde_json::Value {
    let status = json_str(result, &["status"]).unwrap_or("complete");
    chat_summary(
        name,
        status,
        vec![format!(
            "Harness action `{name}` returned status `{status}`."
        )],
    )
}

fn chat_summary(title: &str, status: &str, lines: Vec<String>) -> serde_json::Value {
    json!({
        "title": title,
        "status": status,
        "lines": lines,
    })
}

fn first_failed_blocker(readiness: &serde_json::Value) -> Option<&serde_json::Value> {
    readiness
        .get("checks")
        .and_then(|value| value.as_array())
        .and_then(|checks| {
            checks.iter().find(|check| {
                check.get("status").and_then(|value| value.as_str()) == Some("fail")
                    && check.get("severity").and_then(|value| value.as_str()) == Some("blocker")
            })
        })
}

fn json_array_len(value: &serde_json::Value, path: &[&str]) -> usize {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(|value| value.as_array())
        .map(Vec::len)
        .unwrap_or(0)
}

fn push_first_output_line(lines: &mut Vec<String>, label: &str, output: Option<&str>) {
    let Some(output) = output else {
        return;
    };
    let Some(line) = output.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return;
    };
    lines.push(format!("{label}: {}", truncate_normalized_text(line, 180)));
}

fn parse_chat_named_args(input: &str) -> BTreeMap<String, String> {
    split_chat_tokens(input)
        .into_iter()
        .filter_map(|token| {
            let (key, value) = token.split_once('=')?;
            let key = key.trim().trim_start_matches("--").to_ascii_lowercase();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            if key.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .collect()
}

fn split_chat_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for character in input.chars() {
        match (quote, character) {
            (Some(active), value) if value == active => {
                quote = None;
            }
            (None, '"' | '\'') => {
                quote = Some(character);
            }
            (None, value) if value.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(character),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn chat_arg<'a>(args: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    args.get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

fn infer_chat_project_name(input: &str) -> Option<String> {
    for marker in ["called ", "named "] {
        if let Some(after_marker) = input.split_once(marker).map(|(_, value)| value) {
            let name = after_marker
                .split_once(" using ")
                .map(|(value, _)| value)
                .or_else(|| {
                    after_marker
                        .split_once(" with stack ")
                        .map(|(value, _)| value)
                })
                .or_else(|| after_marker.split_once(" stack ").map(|(value, _)| value))
                .unwrap_or(after_marker)
                .trim()
                .trim_matches('"')
                .trim_matches('\'');

            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }

    None
}

fn infer_chat_stack(input: &str) -> Option<String> {
    let lower = input.to_ascii_lowercase();
    [
        "nextjs-postgres",
        "vite-express-sqlite",
        "fastapi-react",
        "aspnet-blazor",
        "cloudflare-worker",
        "rust-bevy",
    ]
    .iter()
    .find(|stack| lower.contains(**stack))
    .map(|stack| (*stack).to_string())
}

fn summarize_readiness(readiness: &serde_json::Value) -> String {
    let status = readiness
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let blockers = readiness
        .get("blockers")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let warnings = readiness
        .get("warnings")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let can_execute = readiness
        .get("can_execute")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let next = readiness
        .get("checks")
        .and_then(|value| value.as_array())
        .and_then(|checks| {
            checks.iter().find(|check| {
                check.get("status").and_then(|value| value.as_str()) == Some("fail")
                    && check.get("severity").and_then(|value| value.as_str()) == Some("blocker")
            })
        });

    if can_execute {
        return format!("Readiness is `{status}` with no blockers. The harness loop can run.");
    }

    if let Some(check) = next {
        let label = check
            .get("label")
            .and_then(|value| value.as_str())
            .unwrap_or("next blocker");
        let detail = check
            .get("detail")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        return format!(
            "Readiness is `{status}` with {blockers} blocker(s) and {warnings} warning(s). Next blocker: {label}. {detail}"
        );
    }

    format!("Readiness is `{status}` with {blockers} blocker(s) and {warnings} warning(s).")
}

fn chat_command_kind(command: &str) -> Option<ProjectCommandKind> {
    match command {
        "setup" | "run setup" | "setup dry" | "run setup dry" => Some(ProjectCommandKind::Setup),
        "test" | "tests" | "run test" | "run tests" | "test dry" | "tests dry" | "run test dry"
        | "run tests dry" => Some(ProjectCommandKind::Test),
        "build" | "run build" | "build dry" | "run build dry" => Some(ProjectCommandKind::Build),
        _ => None,
    }
}

fn command_kind_name_for_chat(kind: ProjectCommandKind) -> &'static str {
    match kind {
        ProjectCommandKind::Setup => "setup",
        ProjectCommandKind::Dev => "dev",
        ProjectCommandKind::Test => "test",
        ProjectCommandKind::Build => "build",
    }
}

fn parse_chat_host(value: &str) -> Result<HostKind> {
    match value.trim() {
        "codex" => Ok(HostKind::Codex),
        "claude" => Ok(HostKind::Claude),
        "stub" => Ok(HostKind::Stub),
        other => bail!("unsupported host `{other}`; expected codex, claude, or stub"),
    }
}

fn run_harness_chat_safe_repairs(workspace_root: &Path) -> Result<Vec<serde_json::Value>> {
    let readiness = build_readiness(workspace_root)?;
    let mut actions = Vec::new();
    let Some(checks) = readiness.get("checks").and_then(|value| value.as_array()) else {
        return Ok(vec![chat_action("build-readiness", readiness)]);
    };

    for check in checks {
        if check.get("status").and_then(|value| value.as_str()) == Some("pass") {
            continue;
        }

        let action = check
            .get("repair_action")
            .and_then(|value| value.as_str())
            .unwrap_or("manual");
        let result = match action {
            "repair_scaffold" => repair_project(ProjectRepairOptions {
                workspace_root: workspace_root.to_path_buf(),
                scaffold: true,
                force: false,
            })
            .and_then(|result| Ok(serde_json::to_value(result)?)),
            "run_doctor" => doctor_project(ProjectDoctorOptions {
                workspace_root: workspace_root.to_path_buf(),
            })
            .and_then(|result| Ok(serde_json::to_value(result)?)),
            "seed_design_bundle" => {
                seed_design_bundle(workspace_root, DesignBundleSeedRequest { force: false })
            }
            "run_command" => {
                let kind = check
                    .get("repair_payload")
                    .and_then(|value| value.get("kind"))
                    .and_then(|value| value.as_str())
                    .and_then(|value| match value {
                        "setup" => Some(ProjectCommandKind::Setup),
                        "test" => Some(ProjectCommandKind::Test),
                        "build" => Some(ProjectCommandKind::Build),
                        _ => None,
                    });
                match kind {
                    Some(kind) => run_project_command(ProjectRunCommandOptions {
                        workspace_root: workspace_root.to_path_buf(),
                        kind,
                        dry_run: false,
                    })
                    .and_then(|result| Ok(serde_json::to_value(result)?)),
                    None => Ok(json!({
                        "ok": false,
                        "status": "unsupported_command_repair",
                    })),
                }
            }
            "preview_start" => preview_start(ProjectPreviewLifecycleOptions {
                workspace_root: workspace_root.to_path_buf(),
            })
            .and_then(|result| Ok(serde_json::to_value(result)?)),
            "preview_check" => preview_check(ProjectPreviewCheckOptions {
                workspace_root: workspace_root.to_path_buf(),
            })
            .and_then(|result| Ok(serde_json::to_value(result)?)),
            _ => Ok(json!({
                "ok": true,
                "status": "skipped",
                "reason": "manual or queue/execution action",
                "check": check,
            })),
        };

        let value = match result {
            Ok(value) => value,
            Err(error) => json!({
                "ok": false,
                "status": "failed",
                "message": error.to_string(),
                "check": check,
            }),
        };
        actions.push(chat_action(action, value));
    }

    actions.push(chat_action(
        "build-readiness",
        build_readiness(workspace_root)?,
    ));
    Ok(actions)
}

#[derive(Debug, Clone)]
struct DesignDocumentSpec {
    id: &'static str,
    label: &'static str,
    purpose: &'static str,
    path: String,
    min_words: usize,
}

fn design_bundle(workspace_root: &Path) -> Result<serde_json::Value> {
    let capsule_path = workspace_root.join(".mutagen/project.json");
    if !capsule_path.exists() {
        return Ok(json!({
            "ok": false,
            "status": "uninitialized",
            "workspace_root": workspace_root.to_string_lossy(),
            "capsule_path": capsule_path.to_string_lossy(),
            "readiness": {
                "total": 0,
                "ready": 0,
                "draft": 0,
                "missing": 0,
                "percent": 0,
                "all_ready": false,
            },
            "documents": [],
        }));
    }

    let inspect = inspect_project(ProjectInspectOptions {
        workspace_root: workspace_root.to_path_buf(),
    })?;
    let mut documents = Vec::new();
    let mut ready = 0usize;
    let mut draft = 0usize;
    let mut missing = 0usize;

    for spec in design_document_specs(&inspect.capsule) {
        let document = design_document_summary(workspace_root, &spec)?;
        match document
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("draft")
        {
            "ready" => ready += 1,
            "missing" => missing += 1,
            _ => draft += 1,
        }
        documents.push(document);
    }

    let total = documents.len();
    let status = if missing > 0 {
        "attention"
    } else if ready == total {
        "ready"
    } else {
        "draft"
    };
    let percent = if total == 0 { 0 } else { ready * 100 / total };

    Ok(json!({
        "ok": true,
        "status": status,
        "workspace_root": workspace_root.to_string_lossy(),
        "readiness": {
            "total": total,
            "ready": ready,
            "draft": draft,
            "missing": missing,
            "percent": percent,
            "all_ready": ready == total && total > 0,
        },
        "documents": documents,
    }))
}

fn write_design_doc(workspace_root: &Path, request: DesignDocRequest) -> Result<serde_json::Value> {
    let inspect = inspect_project(ProjectInspectOptions {
        workspace_root: workspace_root.to_path_buf(),
    })?;
    let spec = resolve_design_document_spec(&inspect.capsule, &request.document)?;
    let path = workspace_root.join(&spec.path);
    let mut content = request.content.replace("\r\n", "\n");

    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))?;

    Ok(json!({
        "ok": true,
        "status": "updated",
        "document": design_document_summary(workspace_root, &spec)?,
        "bundle": design_bundle(workspace_root)?,
    }))
}

fn seed_design_doc(
    workspace_root: &Path,
    request: DesignDocSeedRequest,
) -> Result<serde_json::Value> {
    let inspect = inspect_project(ProjectInspectOptions {
        workspace_root: workspace_root.to_path_buf(),
    })?;
    let spec = resolve_design_document_spec(&inspect.capsule, &request.document)?;
    let current = design_document_summary(workspace_root, &spec)?;
    let current_status = current
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("draft");

    if current_status == "ready" && !request.force {
        bail!(
            "{} is already ready; refusing to replace it without force because surprises are rude",
            spec.label
        );
    }

    let path = workspace_root.join(&spec.path);
    let direction = design_direction_from_brief(workspace_root, &inspect.capsule)?;
    let content = seeded_design_doc_content(&inspect.capsule, spec.id, &direction)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))?;

    Ok(json!({
        "ok": true,
        "status": "seeded",
        "document": design_document_summary(workspace_root, &spec)?,
        "bundle": design_bundle(workspace_root)?,
    }))
}

fn seed_design_bundle(
    workspace_root: &Path,
    request: DesignBundleSeedRequest,
) -> Result<serde_json::Value> {
    let inspect = inspect_project(ProjectInspectOptions {
        workspace_root: workspace_root.to_path_buf(),
    })?;
    let mut seeded = Vec::new();
    let mut skipped = Vec::new();

    for spec in design_document_specs(&inspect.capsule) {
        let current = design_document_summary(workspace_root, &spec)?;
        let current_status = current
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("draft");

        if current_status == "ready" && !request.force {
            skipped.push(json!({
                "id": spec.id,
                "label": spec.label,
                "status": current_status,
                "path": spec.path,
            }));
            continue;
        }

        let result = seed_design_doc(
            workspace_root,
            DesignDocSeedRequest {
                document: spec.id.to_string(),
                force: request.force,
            },
        )?;
        if let Some(document) = result.get("document") {
            seeded.push(document.clone());
        }
    }

    Ok(json!({
        "ok": true,
        "status": "seeded",
        "seeded": seeded,
        "skipped": skipped,
        "bundle": design_bundle(workspace_root)?,
    }))
}

fn design_document_specs(capsule: &ProjectCapsule) -> Vec<DesignDocumentSpec> {
    vec![
        DesignDocumentSpec {
            id: "brief",
            label: "Design Brief",
            purpose: "Captured project direction from the builder conversation.",
            path: capsule.design.brief.clone(),
            min_words: 8,
        },
        DesignDocumentSpec {
            id: "prd",
            label: "PRD",
            purpose: "Product requirements, goals, acceptance criteria, and open questions.",
            path: capsule.documents.prd.clone(),
            min_words: 24,
        },
        DesignDocumentSpec {
            id: "adr",
            label: "ADR",
            purpose: "Architecture decisions and tradeoffs the implementation should respect.",
            path: capsule.documents.adr.clone(),
            min_words: 24,
        },
        DesignDocumentSpec {
            id: "ddd",
            label: "DDD",
            purpose: "Domain concepts, entities, events, and invariants.",
            path: capsule.documents.ddd.clone(),
            min_words: 24,
        },
        DesignDocumentSpec {
            id: "isc",
            label: "ISC",
            purpose: "Implied systems contract for data, APIs, integrations, and operations.",
            path: capsule.documents.isc.clone(),
            min_words: 24,
        },
        DesignDocumentSpec {
            id: "dsd",
            label: "DSD",
            purpose: "Design style direction for UI, components, tone, and interaction quality.",
            path: capsule.documents.dsd.clone(),
            min_words: 24,
        },
    ]
}

fn resolve_design_document_spec(
    capsule: &ProjectCapsule,
    document: &str,
) -> Result<DesignDocumentSpec> {
    let document_id = normalize_design_document_id(document)?;
    design_document_specs(capsule)
        .into_iter()
        .find(|spec| spec.id == document_id)
        .ok_or_else(|| anyhow::anyhow!("unknown design document `{document}`"))
}

fn normalize_design_document_id(document: &str) -> Result<&'static str> {
    match document.trim().to_ascii_lowercase().as_str() {
        "brief" | "design-brief" | "design_brief" | "project-brief" | "project_brief" => {
            Ok("brief")
        }
        "prd" | "product" | "requirements" | "product-requirements" => Ok("prd"),
        "adr" | "architecture" | "architecture-design-record" => Ok("adr"),
        "ddd" | "domain" | "domain-model" => Ok("ddd"),
        "isc" | "systems-contract" | "implied-systems-contract" => Ok("isc"),
        "dsd" | "design-style-guide" | "style-guide" => Ok("dsd"),
        "" => bail!("design document is required"),
        other => bail!(
            "unsupported design document `{}`; expected brief, prd, adr, ddd, isc, or dsd",
            other
        ),
    }
}

fn design_document_summary(
    workspace_root: &Path,
    spec: &DesignDocumentSpec,
) -> Result<serde_json::Value> {
    let path = workspace_root.join(&spec.path);
    let exists = path.exists();
    let content = if exists {
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?
    } else {
        String::new()
    };
    let meaningful_text = design_document_meaningful_text(&content);
    let word_count = meaningful_text.split_whitespace().count();
    let status = if !exists {
        "missing"
    } else if word_count >= spec.min_words {
        "ready"
    } else {
        "draft"
    };
    let metadata = fs::metadata(&path).ok();
    let updated_at = metadata
        .as_ref()
        .and_then(|value| value.modified().ok())
        .and_then(|value| system_time_rfc3339(value).ok());
    let byte_count = metadata.as_ref().map(|value| value.len()).unwrap_or(0);
    let excerpt = truncate_normalized_text(&meaningful_text, 320);

    Ok(json!({
        "id": spec.id,
        "label": spec.label,
        "purpose": spec.purpose,
        "path": spec.path,
        "exists": exists,
        "status": status,
        "byte_count": byte_count,
        "word_count": word_count,
        "min_words": spec.min_words,
        "updated_at": updated_at,
        "excerpt": excerpt,
        "content": content,
    }))
}

fn design_direction_from_brief(workspace_root: &Path, capsule: &ProjectCapsule) -> Result<String> {
    let brief_path = workspace_root.join(&capsule.design.brief);
    if !brief_path.exists() {
        return Ok(String::new());
    }

    let brief = fs::read_to_string(&brief_path)
        .with_context(|| format!("failed to read {}", brief_path.display()))?;
    Ok(truncate_normalized_text(
        &design_document_meaningful_text(&brief),
        900,
    ))
}

fn seeded_design_doc_content(
    capsule: &ProjectCapsule,
    document_id: &str,
    direction: &str,
) -> Result<String> {
    let project = capsule.name.trim();
    let stack = capsule.stack.trim();
    let design_system = capsule.design_system.trim();
    let deploy_target = capsule.deploy_target.as_deref().unwrap_or("local");
    let direction = if direction.trim().is_empty() {
        "No product direction has been captured yet. Start by writing the project goal in the builder conversation, then save or queue it."
    } else {
        direction.trim()
    };

    let content = match document_id {
        "brief" => format!(
            "# Design Brief\n\n## Current direction\n\n{direction}\n\n## Intake log\n\n### {} - Seeded from dashboard\n\nSeeded from the design bundle workbench.\n",
            now_rfc3339()?
        ),
        "prd" => format!(
            "# Product Requirements Document\n\n## Product Direction\n\n{direction}\n\n## Users\n\n- Primary: people using {project} to complete the core workflow.\n- Secondary: operators maintaining the generated application.\n\n## Scope\n\n- Build the first usable version on the {stack} blueprint.\n- Preserve the {design_system} design system unless the brief says otherwise.\n- Keep the workflow understandable from the dashboard, backlog, and preview loop.\n\n## Acceptance Criteria\n\n- The project can be installed, run, tested, and built through the harness.\n- The first queued request maps back to this PRD without mystery archaeology.\n- Preview behavior is observable from the dashboard.\n\n## Open Questions\n\n- Which integrations are mandatory for the first useful release?\n- Which data needs persistence, auditability, or import/export support?\n- What should be considered out of scope for the first build?\n"
        ),
        "adr" => format!(
            "# Architecture Design Record\n\n## Context\n\n{project} is generated through the harness using the {stack} blueprint and targets {deploy_target} deployment.\n\n## Decision\n\n- Use the generated project scaffold as the implementation boundary.\n- Keep feature work sliced through the harness queue instead of hand-editing a parallel plan.\n- Treat dashboard actions as the operator control surface for build, preview, and execution.\n\n## Consequences\n\n- Project direction stays visible in the design bundle before implementation starts.\n- Implementation work can be resumed from queue state instead of tribal memory and caffeine residue.\n- Any architecture change should update this ADR before queued slices depend on it.\n\n## Follow-ups\n\n- Confirm persistence, authentication, and deployment choices before production-facing work.\n- Record external service contracts in the ISC.\n"
        ),
        "ddd" => format!(
            "# Domain Model\n\n## Source Direction\n\n{direction}\n\n## Core Concepts\n\n- User: the person completing the primary workflow in {project}.\n- Workspace: the project state, documents, generated app, and execution queue.\n- Request: a natural-language product change translated into executable slices.\n- Slice: a scoped unit of implementation, verification, and review.\n\n## Invariants\n\n- Requests should remain traceable to builder conversation turns and design documents.\n- Slices should stay small enough to inspect and recover.\n- Generated behavior should preserve the project design system and stack assumptions.\n\n## Events\n\n- Direction captured.\n- Document updated.\n- Request queued.\n- Slice prepared.\n- Slice finalized.\n"
        ),
        "isc" => format!(
            "# Implied Systems Contract\n\n## Runtime Contract\n\n- Stack: {stack}.\n- Design system: {design_system}.\n- Deploy target: {deploy_target}.\n- Harness state lives under `.mutagen` and queue state lives under `slices`.\n\n## Dashboard Contract\n\n- Builder messages may be saved as notes, design direction, or queued work.\n- Design documents are editable through the design bundle workbench.\n- Build and preview actions should report command output through harness logs.\n\n## Data Contract\n\n- Project capsule paths remain the source of truth for generated artifacts.\n- Queue entries must stay valid against the harness queue schema.\n- Generated application data contracts should be added here before implementation depends on them.\n\n## Operational Contract\n\n- A project is not considered ready until setup, test, build, and preview checks can be observed.\n"
        ),
        "dsd" => format!(
            "# Design Style Guide\n\n## Product Direction\n\n{direction}\n\n## Visual System\n\n- Use {design_system} as the baseline design system.\n- Favor dense, legible operational screens over marketing surfaces unless the product brief demands otherwise.\n- Keep dashboard and generated app interactions explicit, reversible where practical, and easy to scan.\n\n## Components\n\n- Buttons should represent clear commands.\n- Forms should group related fields and expose validation directly.\n- Status indicators should use stable labels that match harness states.\n\n## Interaction Quality\n\n- Primary workflows should be available without reading documentation first.\n- Empty states should tell the operator what can happen next.\n- Error states should explain the failed action and the next recovery path.\n"
        ),
        _ => unreachable!("design document id was normalized"),
    };

    Ok(content)
}

fn design_document_meaningful_text(content: &str) -> String {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .filter(|line| !line.starts_with("---"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_normalized_text(value: &str, limit: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= limit {
        return normalized;
    }

    let take = limit.saturating_sub(3);
    let truncated = normalized.chars().take(take).collect::<String>();
    format!("{}...", truncated.trim_end())
}

fn build_readiness(workspace_root: &Path) -> Result<serde_json::Value> {
    let capsule_path = workspace_root.join(".mutagen/project.json");
    if !capsule_path.exists() {
        return Ok(json!({
            "ok": false,
            "status": "uninitialized",
            "can_execute": false,
            "workspace_root": workspace_root.to_string_lossy(),
            "capsule_path": capsule_path.to_string_lossy(),
            "blockers": 1,
            "warnings": 0,
            "passed": 0,
            "checks": [
                readiness_check(
                    "capsule",
                    "workspace",
                    "Project capsule",
                    "fail",
                    "blocker",
                    "Create the project capsule before running execution.",
                    "Create Project",
                )
            ],
        }));
    }

    let dashboard = dashboard_project(ProjectDashboardOptions {
        workspace_root: workspace_root.to_path_buf(),
    })?;
    let inspect = inspect_project(ProjectInspectOptions {
        workspace_root: workspace_root.to_path_buf(),
    })?;
    let design = design_bundle(workspace_root)?;
    let queue = queue_status(workspace_root)?;
    let preview_plan = match preview_plan(ProjectPreviewPlanOptions {
        workspace_root: workspace_root.to_path_buf(),
    }) {
        Ok(plan) => json!(plan),
        Err(error) => json!({
            "ok": false,
            "status": "unconfigured",
            "workspace_root": workspace_root.to_string_lossy(),
            "stack": inspect.capsule.stack,
            "url": inspect.capsule.preview.url,
            "command_kind": inspect.capsule.preview.command_kind,
            "command": "",
            "readiness_timeout_seconds": inspect.capsule.preview.readiness_timeout_seconds,
            "detail": error.to_string(),
        }),
    };
    let preview_check = match preview_check(ProjectPreviewCheckOptions {
        workspace_root: workspace_root.to_path_buf(),
    }) {
        Ok(check) => json!(check),
        Err(error) => json!({
            "ok": false,
            "status": "skipped",
            "mode": "unknown",
            "running": false,
            "ready": false,
            "url": json_str(&preview_plan, &["url"]).unwrap_or(""),
            "command": json_str(&preview_plan, &["command"]).unwrap_or(""),
            "state_path": workspace_root.join(".mutagen/state/preview.json").to_string_lossy(),
            "log_path": workspace_root.join(".mutagen/state/preview-output.log").to_string_lossy(),
            "detail": error.to_string(),
        }),
    };
    let build_log_path = workspace_root.join(&inspect.capsule.state.build_log);
    let build_log_entries = read_jsonl_tail(&build_log_path, 1000)?;
    let mut checks = Vec::new();

    checks.push(readiness_check(
        "capsule",
        "workspace",
        "Project capsule",
        if dashboard.project.capsule_ok {
            "pass"
        } else {
            "fail"
        },
        "blocker",
        if dashboard.project.capsule_ok {
            "Capsule paths are present."
        } else {
            "One or more capsule-managed paths are missing."
        },
        "Repair Capsule",
    ));
    checks.push(readiness_check(
        "scaffold",
        "workspace",
        "Generated scaffold",
        if dashboard.project.scaffold_ok {
            "pass"
        } else {
            "fail"
        },
        "blocker",
        if dashboard.project.scaffold_ok {
            "Generated scaffold files are present."
        } else {
            "One or more generated scaffold files are missing."
        },
        "Repair Scaffold",
    ));
    checks.push(readiness_check(
        "tooling",
        "workspace",
        "Tooling",
        if dashboard.project.doctor_ok {
            "pass"
        } else {
            "fail"
        },
        "blocker",
        if dashboard.project.doctor_ok {
            "Required project tools are available."
        } else {
            "Doctor reported missing tools for this stack."
        },
        "Run Doctor",
    ));

    let design_ready = json_bool(&design, &["readiness", "all_ready"]);
    let design_ready_count = json_usize(&design, &["readiness", "ready"]);
    let design_total = json_usize(&design, &["readiness", "total"]);
    let design_draft = json_usize(&design, &["readiness", "draft"]);
    let design_missing = json_usize(&design, &["readiness", "missing"]);
    checks.push(readiness_check(
        "design_bundle",
        "design",
        "Design bundle",
        if design_ready { "pass" } else { "fail" },
        "blocker",
        &format!(
            "{design_ready_count}/{design_total} documents ready; {design_draft} draft and {design_missing} missing."
        ),
        "Complete Design Bundle",
    ));

    for (id, label) in [
        ("setup", "Setup command"),
        ("test", "Test command"),
        ("build", "Build command"),
    ] {
        let latest = latest_command_entry(&build_log_entries, id);
        let passed = latest
            .and_then(|entry| entry.get("status").and_then(|value| value.as_str()))
            .map(|status| status == "completed")
            .unwrap_or(false);
        let detail = latest
            .map(command_entry_detail)
            .unwrap_or_else(|| "No successful run has been recorded yet.".to_string());
        checks.push(readiness_check(
            &format!("command_{id}"),
            "build",
            label,
            if passed { "pass" } else { "fail" },
            "blocker",
            &detail,
            &format!("Run {}", label.trim_end_matches(" command")),
        ));
    }

    let preview_configured = json_bool(&preview_plan, &["ok"])
        && !json_str(&preview_plan, &["command"])
            .unwrap_or("")
            .trim()
            .is_empty()
        && !json_str(&preview_plan, &["url"])
            .unwrap_or("")
            .trim()
            .is_empty();
    let preview_config_detail = json_str(&preview_plan, &["detail"]).unwrap_or_else(|| {
        if preview_configured {
            "Preview command and URL are configured."
        } else {
            "Preview command or URL is missing from the active blueprint."
        }
    });
    checks.push(readiness_check(
        "preview_config",
        "preview",
        "Preview configuration",
        if preview_configured { "pass" } else { "fail" },
        "blocker",
        preview_config_detail,
        "Apply Blueprint",
    ));
    checks.push(readiness_check(
        "preview_reachable",
        "preview",
        "Preview reachability",
        if json_bool(&preview_check, &["ok"]) {
            "pass"
        } else {
            "warning"
        },
        "warning",
        json_str(&preview_check, &["detail"]).unwrap_or("Preview reachability was not checked."),
        "Start Preview",
    ));

    let queue_valid = json_bool(&queue, &["validation", "ok"]);
    checks.push(readiness_check(
        "queue_valid",
        "queue",
        "Queue validation",
        if queue_valid { "pass" } else { "fail" },
        "blocker",
        if queue_valid {
            "Queue schema validation is clean."
        } else {
            "Queue validation reported errors."
        },
        "Fix Queue",
    ));

    let selection_status = json_str(&queue, &["selection", "status"]).unwrap_or("unknown");
    let active_slice_id = queue
        .get("active_slice_id")
        .and_then(|value| value.as_str());
    let queue_executable = selection_status == "ready" || active_slice_id.is_some();
    let queue_detail = match active_slice_id {
        Some(slice_id) => format!("Slice {slice_id} is already active."),
        None if selection_status == "ready" => "A ready slice is available.".to_string(),
        None if selection_status == "queue_clear" => {
            "The queue is clear; add or queue work first.".to_string()
        }
        None if selection_status == "stalled" => {
            "The queue is stalled on dependencies.".to_string()
        }
        None => format!("Queue selection status is {selection_status}."),
    };
    checks.push(readiness_check(
        "queue_ready",
        "queue",
        "Executable queue state",
        if queue_executable { "pass" } else { "fail" },
        "blocker",
        &queue_detail,
        "Queue Work",
    ));

    let blockers = checks
        .iter()
        .filter(|check| {
            check.get("status").and_then(|value| value.as_str()) == Some("fail")
                && check.get("severity").and_then(|value| value.as_str()) == Some("blocker")
        })
        .count();
    let warnings = checks
        .iter()
        .filter(|check| check.get("status").and_then(|value| value.as_str()) == Some("warning"))
        .count();
    let passed = checks
        .iter()
        .filter(|check| check.get("status").and_then(|value| value.as_str()) == Some("pass"))
        .count();
    let status = if blockers > 0 {
        "blocked"
    } else if warnings > 0 {
        "warning"
    } else {
        "ready"
    };

    Ok(json!({
        "ok": true,
        "status": status,
        "can_execute": blockers == 0,
        "workspace_root": workspace_root.to_string_lossy(),
        "build_log_path": build_log_path.to_string_lossy(),
        "blockers": blockers,
        "warnings": warnings,
        "passed": passed,
        "checks": checks,
        "design_bundle": design,
        "queue": queue,
        "preview": {
            "plan": preview_plan,
            "check": preview_check,
        },
    }))
}

fn readiness_check(
    id: &str,
    category: &str,
    label: &str,
    status: &str,
    severity: &str,
    detail: &str,
    action: &str,
) -> serde_json::Value {
    let (repair_action, repair_payload) = readiness_repair_metadata(id);

    json!({
        "id": id,
        "category": category,
        "label": label,
        "status": status,
        "severity": severity,
        "detail": detail,
        "action": action,
        "repair_action": repair_action,
        "repair_payload": repair_payload,
    })
}

fn readiness_repair_metadata(id: &str) -> (&'static str, serde_json::Value) {
    match id {
        "scaffold" => ("repair_scaffold", json!({})),
        "tooling" => ("run_doctor", json!({})),
        "design_bundle" => ("seed_design_bundle", json!({ "force": false })),
        "command_setup" => ("run_command", json!({ "kind": "setup" })),
        "command_test" => ("run_command", json!({ "kind": "test" })),
        "command_build" => ("run_command", json!({ "kind": "build" })),
        "preview_reachable" => ("preview_start", json!({})),
        "queue_ready" => ("focus_builder_queue_work", json!({})),
        _ => ("manual", serde_json::Value::Null),
    }
}

fn latest_command_entry<'a>(
    entries: &'a [serde_json::Value],
    kind: &str,
) -> Option<&'a serde_json::Value> {
    entries.iter().rev().find(|entry| {
        entry
            .get("command_kind")
            .and_then(|value| value.as_str())
            .map(|value| value == kind)
            .unwrap_or(false)
    })
}

fn command_entry_detail(entry: &serde_json::Value) -> String {
    let status = entry
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let recorded_at = entry
        .get("recorded_at")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown time");
    let exit_code = entry
        .get("exit_code")
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string());

    format!("Latest run {status} at {recorded_at} with exit code {exit_code}.")
}

fn json_bool(value: &serde_json::Value, path: &[&str]) -> bool {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn json_usize(value: &serde_json::Value, path: &[&str]) -> usize {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or(0)
}

fn json_str<'a>(value: &'a serde_json::Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .and_then(|value| value.as_str())
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

fn dashboard_settings_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".mutagen/state/dashboard-settings.json")
}

fn read_dashboard_settings(workspace_root: &Path) -> Result<Option<DashboardInferenceSettings>> {
    let path = dashboard_settings_path(workspace_root);
    if !path.exists() {
        return Ok(None);
    }

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let settings: DashboardInferenceSettings = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(settings))
}

fn write_dashboard_settings(
    workspace_root: &Path,
    settings: &DashboardInferenceSettings,
) -> Result<PathBuf> {
    let path = dashboard_settings_path(workspace_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_string_pretty(settings)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn inference_host_options(default_host: HostKind, selected_host: HostKind) -> Vec<HostKind> {
    let mut hosts = vec![HostKind::Codex, HostKind::Claude];

    if default_host == HostKind::Stub || selected_host == HostKind::Stub {
        hosts.push(HostKind::Stub);
    }

    hosts
}

fn resolve_dashboard_inference_host(
    workspace_root: &Path,
    default_host: HostKind,
) -> Result<HostKind> {
    Ok(read_dashboard_settings(workspace_root)?
        .map(|settings| settings.host)
        .unwrap_or(default_host))
}

fn inference_host_state(
    workspace_root: &Path,
    default_host: HostKind,
) -> Result<serde_json::Value> {
    let settings = read_dashboard_settings(workspace_root)?;
    let selected_host = settings
        .as_ref()
        .map(|value| value.host)
        .unwrap_or(default_host);
    let workflow_config_path = workspace_root.join(".claude/workflow.json");
    let workflow_config = load_workflow_config_file(&workflow_config_path)?;
    let profile = resolved_host_profile(selected_host, &workflow_config);
    let settings_path = dashboard_settings_path(workspace_root);

    Ok(json!({
        "ok": true,
        "status": "ready",
        "selected_host": selected_host,
        "default_host": default_host,
        "persisted": settings.is_some(),
        "updated_at": settings.as_ref().map(|value| value.updated_at.clone()),
        "settings_path": settings_path.to_string_lossy(),
        "available_hosts": inference_host_options(default_host, selected_host),
        "profile": profile,
    }))
}

fn set_inference_host(
    workspace_root: &Path,
    default_host: HostKind,
    host: HostKind,
) -> Result<serde_json::Value> {
    let settings = DashboardInferenceSettings {
        host,
        updated_at: now_rfc3339()?,
    };
    let settings_path = write_dashboard_settings(workspace_root, &settings)?;
    let mut state = inference_host_state(workspace_root, default_host)?;
    if let Some(object) = state.as_object_mut() {
        object.insert(
            "status".to_string(),
            serde_json::Value::String("updated".to_string()),
        );
        object.insert(
            "settings_path".to_string(),
            serde_json::Value::String(settings_path.to_string_lossy().into_owned()),
        );
    }
    Ok(state)
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

fn append_jsonl(path: &Path, value: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(value)?)
        .with_context(|| format!("failed to append {}", path.display()))
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

fn terminal_jobs_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".mutagen/state/dashboard-terminal")
}

fn terminal_job_path(workspace_root: &Path, id: &str) -> Result<PathBuf> {
    validate_terminal_job_id(id)?;
    Ok(terminal_jobs_root(workspace_root).join(format!("{id}.json")))
}

fn terminal_log_paths(workspace_root: &Path, id: &str) -> Result<(PathBuf, PathBuf)> {
    validate_terminal_job_id(id)?;
    let root = terminal_jobs_root(workspace_root);
    Ok((
        root.join(format!("{id}.stdout.log")),
        root.join(format!("{id}.stderr.log")),
    ))
}

fn validate_terminal_job_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("invalid terminal job id `{id}`");
    }
    Ok(())
}

fn start_terminal_job(
    workspace_root: &Path,
    request: TerminalCommandRequest,
) -> Result<serde_json::Value> {
    let command = request.command.trim();
    if command.is_empty() {
        bail!("terminal command is required");
    }

    let jobs_root = terminal_jobs_root(workspace_root);
    fs::create_dir_all(&jobs_root)
        .with_context(|| format!("failed to create {}", jobs_root.display()))?;

    let id = format!("terminal-{}", unix_millis()?);
    let metadata_path = terminal_job_path(workspace_root, &id)?;
    let (stdout_path, stderr_path) = terminal_log_paths(workspace_root, &id)?;
    let started_at = now_rfc3339()?;
    let stdout = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&stdout_path)
        .with_context(|| format!("failed to open {}", stdout_path.display()))?;
    let stderr = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&stderr_path)
        .with_context(|| format!("failed to open {}", stderr_path.display()))?;

    let mut child_command = Command::new("bash");
    child_command
        .arg("-lc")
        .arg(command)
        .current_dir(workspace_root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .env("MUTAGEN_WORKSPACE_ROOT", workspace_root);

    if let Ok(harness_bin) = std::env::current_exe() {
        child_command.env("MUTAGEN_HARNESS_BIN", harness_bin);
    }
    if let Ok(runner_script) = runner_script_path() {
        child_command.env("MUTAGEN_RUN_EXECUTE_NEXT", runner_script);
    }

    let mut child = child_command
        .spawn()
        .with_context(|| format!("failed to launch terminal command `{command}`"))?;
    let pid = child.id();

    let job = TerminalJob {
        id: id.clone(),
        ok: true,
        status: "running".to_string(),
        workspace_root: workspace_root.to_string_lossy().into_owned(),
        cwd: workspace_root.to_string_lossy().into_owned(),
        shell: "bash -lc".to_string(),
        command: command.to_string(),
        pid: Some(pid),
        started_at,
        ended_at: None,
        updated_at: now_rfc3339()?,
        stdout_path: stdout_path.to_string_lossy().into_owned(),
        stderr_path: stderr_path.to_string_lossy().into_owned(),
        metadata_path: metadata_path.to_string_lossy().into_owned(),
        exit_code: None,
        message: Some("Terminal command started from the dashboard.".to_string()),
    };
    write_terminal_job(&job)?;

    thread::spawn(move || match child.wait() {
        Ok(status) => {
            if let Err(error) = finish_terminal_job(metadata_path, status.code()) {
                eprintln!("failed to finish terminal job: {error}");
            }
        }
        Err(error) => {
            if let Err(write_error) = fail_terminal_job(metadata_path, error.to_string()) {
                eprintln!("failed to record terminal job failure: {write_error}");
            }
        }
    });

    Ok(json!({
        "ok": true,
        "status": "started",
        "job": job,
        "messages": terminal_messages(&[job], 40)?,
    }))
}

fn terminal_jobs(
    workspace_root: &Path,
    limit: usize,
    log_limit: usize,
) -> Result<serde_json::Value> {
    let mut jobs = read_terminal_jobs(workspace_root)?;
    jobs.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    jobs.truncate(limit);
    let current = jobs.iter().find(|job| job.status == "running").cloned();
    let mut chronological = jobs.clone();
    chronological.sort_by(|left, right| left.started_at.cmp(&right.started_at));

    Ok(json!({
        "ok": true,
        "status": "ready",
        "jobs_root": terminal_jobs_root(workspace_root).to_string_lossy(),
        "current": current,
        "jobs": jobs,
        "messages": terminal_messages(&chronological, log_limit)?,
    }))
}

fn cancel_terminal_job(workspace_root: &Path, id: &str) -> Result<serde_json::Value> {
    let mut job = read_terminal_job(workspace_root, id)?;
    if job.status != "running" {
        return Ok(json!({
            "ok": false,
            "status": "not_cancellable",
            "job": job,
            "messages": terminal_messages(&[job], 40)?,
        }));
    }

    if let Some(pid) = job.pid {
        let _ = Command::new("bash")
            .arg("-lc")
            .arg(format!("kill -TERM {pid}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    let ended_at = now_rfc3339()?;
    job.ok = false;
    job.status = "cancelled".to_string();
    job.ended_at = Some(ended_at.clone());
    job.updated_at = ended_at;
    job.message = Some("Terminal command cancelled from the dashboard.".to_string());
    write_terminal_job(&job)?;

    Ok(json!({
        "ok": true,
        "status": "cancelled",
        "job": job,
        "messages": terminal_messages(&[job], 40)?,
    }))
}

fn read_terminal_jobs(workspace_root: &Path) -> Result<Vec<TerminalJob>> {
    let jobs_root = terminal_jobs_root(workspace_root);
    if !jobs_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut jobs = Vec::new();
    for entry in fs::read_dir(&jobs_root)
        .with_context(|| format!("failed to read {}", jobs_root.display()))?
    {
        let path = entry
            .with_context(|| format!("failed to read entry from {}", jobs_root.display()))?
            .path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let job: TerminalJob = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        jobs.push(job);
    }

    Ok(jobs)
}

fn read_terminal_job(workspace_root: &Path, id: &str) -> Result<TerminalJob> {
    let path = terminal_job_path(workspace_root, id)?;
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn write_terminal_job(job: &TerminalJob) -> Result<()> {
    let path = Path::new(&job.metadata_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_string_pretty(job)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn finish_terminal_job(metadata_path: PathBuf, exit_code: Option<i32>) -> Result<()> {
    let raw = fs::read_to_string(&metadata_path)
        .with_context(|| format!("failed to read {}", metadata_path.display()))?;
    let mut job: TerminalJob = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", metadata_path.display()))?;

    if job.status == "cancelled" {
        return Ok(());
    }

    let ended_at = now_rfc3339()?;
    job.ok = exit_code == Some(0);
    job.status = if job.ok { "completed" } else { "failed" }.to_string();
    job.ended_at = Some(ended_at.clone());
    job.updated_at = ended_at;
    job.exit_code = exit_code;
    write_terminal_job(&job)
}

fn fail_terminal_job(metadata_path: PathBuf, message: String) -> Result<()> {
    let raw = fs::read_to_string(&metadata_path)
        .with_context(|| format!("failed to read {}", metadata_path.display()))?;
    let mut job: TerminalJob = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", metadata_path.display()))?;
    let ended_at = now_rfc3339()?;
    job.ok = false;
    job.status = "failed".to_string();
    job.ended_at = Some(ended_at.clone());
    job.updated_at = ended_at;
    job.message = Some(message);
    write_terminal_job(&job)
}

fn terminal_messages(jobs: &[TerminalJob], log_limit: usize) -> Result<Vec<serde_json::Value>> {
    let mut messages = Vec::new();
    for job in jobs {
        messages.push(json!({
            "role": "user",
            "content": format!("$ {}", job.command),
            "created_at": job.started_at,
        }));
        messages.push(json!({
            "role": "assistant",
            "content": terminal_job_content(job),
            "created_at": job.updated_at,
            "status": job.status,
            "actions": [chat_action("terminal-command", terminal_job_result(job, log_limit)?)]
        }));
    }
    Ok(messages)
}

fn terminal_job_content(job: &TerminalJob) -> String {
    match job.status.as_str() {
        "running" => format!("Running `{}` as `{}`.", job.command, job.id),
        "completed" => format!("Command `{}` completed successfully.", job.id),
        "cancelled" => format!("Command `{}` was cancelled.", job.id),
        "failed" => format!("Command `{}` failed.", job.id),
        status => format!("Command `{}` is `{status}`.", job.id),
    }
}

fn terminal_job_result(job: &TerminalJob, log_limit: usize) -> Result<serde_json::Value> {
    let stdout_lines = read_text_tail(Path::new(&job.stdout_path), log_limit)?;
    let stderr_lines = read_text_tail(Path::new(&job.stderr_path), log_limit)?;
    Ok(json!({
        "ok": job.ok,
        "status": job.status,
        "id": job.id,
        "cwd": job.cwd,
        "shell": job.shell,
        "command": job.command,
        "pid": job.pid,
        "started_at": job.started_at,
        "ended_at": job.ended_at,
        "updated_at": job.updated_at,
        "exit_code": job.exit_code,
        "stdout_path": job.stdout_path,
        "stderr_path": job.stderr_path,
        "stdout_lines": stdout_lines,
        "stderr_lines": stderr_lines,
        "message": job.message,
    }))
}

fn execution_jobs_root(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".mutagen/state/dashboard-jobs")
}

fn execution_job_path(workspace_root: &Path, id: &str) -> Result<PathBuf> {
    validate_execution_job_id(id)?;
    Ok(execution_jobs_root(workspace_root).join(format!("{id}.json")))
}

fn execution_log_paths(workspace_root: &Path, id: &str) -> Result<(PathBuf, PathBuf)> {
    validate_execution_job_id(id)?;
    let root = execution_jobs_root(workspace_root);
    Ok((
        root.join(format!("{id}.stdout.log")),
        root.join(format!("{id}.stderr.log")),
    ))
}

fn validate_execution_job_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("invalid execution job id `{id}`");
    }
    Ok(())
}

fn start_execution_job(
    workspace_root: &Path,
    host: HostKind,
    dry_run: bool,
) -> Result<serde_json::Value> {
    if let Some(job) = running_execution_job(workspace_root)? {
        return Ok(json!({
            "ok": false,
            "status": "already_running",
            "message": "An execution job is already running.",
            "job": job,
        }));
    }

    let jobs_root = execution_jobs_root(workspace_root);
    fs::create_dir_all(&jobs_root)
        .with_context(|| format!("failed to create {}", jobs_root.display()))?;

    let id = format!("execution-{}", unix_millis()?);
    let metadata_path = execution_job_path(workspace_root, &id)?;
    let (stdout_path, stderr_path) = execution_log_paths(workspace_root, &id)?;
    let started_at = now_rfc3339()?;
    let host_name = host_name(host).to_string();
    let command = vec![
        "bash".to_string(),
        runner_script_path()?.to_string_lossy().into_owned(),
        "--workspace-root".to_string(),
        workspace_root.to_string_lossy().into_owned(),
        "--host".to_string(),
        host_name,
    ];

    if dry_run {
        let terminal = json!({
            "ok": true,
            "status": "queue_clear",
            "completed_count": 0,
            "dry_run": true,
        });
        fs::write(&stdout_path, serde_json::to_string_pretty(&terminal)?)
            .with_context(|| format!("failed to write {}", stdout_path.display()))?;
        fs::write(&stderr_path, "")
            .with_context(|| format!("failed to write {}", stderr_path.display()))?;
        let ended_at = now_rfc3339()?;
        let job = ExecutionJob {
            id,
            ok: true,
            status: "queue_clear".to_string(),
            workspace_root: workspace_root.to_string_lossy().into_owned(),
            host,
            dry_run,
            pid: None,
            command,
            started_at,
            ended_at: Some(ended_at.clone()),
            updated_at: ended_at,
            stdout_path: stdout_path.to_string_lossy().into_owned(),
            stderr_path: stderr_path.to_string_lossy().into_owned(),
            metadata_path: metadata_path.to_string_lossy().into_owned(),
            exit_code: Some(0),
            completed_count: 0,
            terminal: Some(terminal),
            message: Some(
                "Dry-run execution job recorded without launching the runner.".to_string(),
            ),
        };
        write_execution_job(&job)?;
        return Ok(json!({
            "ok": true,
            "status": "started",
            "job": job,
        }));
    }

    let stdout = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&stdout_path)
        .with_context(|| format!("failed to open {}", stdout_path.display()))?;
    let stderr = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&stderr_path)
        .with_context(|| format!("failed to open {}", stderr_path.display()))?;
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(workspace_root)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .with_context(|| format!("failed to launch {}", command.join(" ")))?;
    let pid = child.id();

    let job = ExecutionJob {
        id: id.clone(),
        ok: true,
        status: "running".to_string(),
        workspace_root: workspace_root.to_string_lossy().into_owned(),
        host,
        dry_run,
        pid: Some(pid),
        command: command.clone(),
        started_at,
        ended_at: None,
        updated_at: now_rfc3339()?,
        stdout_path: stdout_path.to_string_lossy().into_owned(),
        stderr_path: stderr_path.to_string_lossy().into_owned(),
        metadata_path: metadata_path.to_string_lossy().into_owned(),
        exit_code: None,
        completed_count: 0,
        terminal: None,
        message: None,
    };
    write_execution_job(&job)?;

    thread::spawn(move || match child.wait() {
        Ok(status) => {
            if let Err(error) = finish_execution_job(metadata_path, stdout_path, status.code()) {
                eprintln!("failed to finish execution job: {error}");
            }
        }
        Err(error) => {
            if let Err(write_error) = fail_execution_job(metadata_path, error.to_string()) {
                eprintln!("failed to record execution job failure: {write_error}");
            }
        }
    });

    Ok(json!({
        "ok": true,
        "status": "started",
        "job": job,
    }))
}

fn execution_jobs(workspace_root: &Path, limit: usize) -> Result<serde_json::Value> {
    let mut jobs = read_execution_jobs(workspace_root)?;
    jobs.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    jobs.truncate(limit);
    let current = jobs.iter().find(|job| job.status == "running").cloned();

    Ok(json!({
        "ok": true,
        "status": "ready",
        "jobs_root": execution_jobs_root(workspace_root).to_string_lossy(),
        "current": current,
        "jobs": jobs,
    }))
}

fn execution_job_detail(
    workspace_root: &Path,
    id: &str,
    log_limit: usize,
) -> Result<serde_json::Value> {
    let job = read_execution_job(workspace_root, id)?;
    let stdout_lines = read_text_tail(Path::new(&job.stdout_path), log_limit)?;
    let stderr_lines = read_text_tail(Path::new(&job.stderr_path), log_limit)?;

    Ok(json!({
        "ok": true,
        "status": "ready",
        "job": job,
        "stdout_lines": stdout_lines,
        "stderr_lines": stderr_lines,
    }))
}

fn cancel_execution_job(workspace_root: &Path, id: &str) -> Result<serde_json::Value> {
    let mut job = read_execution_job(workspace_root, id)?;
    if job.status != "running" {
        return Ok(json!({
            "ok": false,
            "status": "not_cancellable",
            "job": job,
        }));
    }

    if let Some(pid) = job.pid {
        let _ = Command::new("bash")
            .arg("-lc")
            .arg(format!("kill -TERM {pid}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    let ended_at = now_rfc3339()?;
    job.ok = false;
    job.status = "cancelled".to_string();
    job.ended_at = Some(ended_at.clone());
    job.updated_at = ended_at;
    job.message = Some("Execution job cancelled from the dashboard.".to_string());
    write_execution_job(&job)?;

    Ok(json!({
        "ok": true,
        "status": "cancelled",
        "job": job,
    }))
}

fn running_execution_job(workspace_root: &Path) -> Result<Option<ExecutionJob>> {
    Ok(read_execution_jobs(workspace_root)?
        .into_iter()
        .find(|job| job.status == "running"))
}

fn read_execution_jobs(workspace_root: &Path) -> Result<Vec<ExecutionJob>> {
    let jobs_root = execution_jobs_root(workspace_root);
    if !jobs_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut jobs = Vec::new();
    for entry in fs::read_dir(&jobs_root)
        .with_context(|| format!("failed to read {}", jobs_root.display()))?
    {
        let path = entry
            .with_context(|| format!("failed to read entry from {}", jobs_root.display()))?
            .path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let job: ExecutionJob = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        jobs.push(job);
    }

    Ok(jobs)
}

fn read_execution_job(workspace_root: &Path, id: &str) -> Result<ExecutionJob> {
    let path = execution_job_path(workspace_root, id)?;
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn write_execution_job(job: &ExecutionJob) -> Result<()> {
    let path = Path::new(&job.metadata_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_string_pretty(job)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

fn finish_execution_job(
    metadata_path: PathBuf,
    stdout_path: PathBuf,
    exit_code: Option<i32>,
) -> Result<()> {
    let raw = fs::read_to_string(&metadata_path)
        .with_context(|| format!("failed to read {}", metadata_path.display()))?;
    let mut job: ExecutionJob = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", metadata_path.display()))?;

    if job.status == "cancelled" {
        return Ok(());
    }

    let stdout = fs::read_to_string(&stdout_path).unwrap_or_default();
    let terminal = parse_runner_terminal(&stdout);
    let status = execution_terminal_status(exit_code, terminal.as_ref());
    let ended_at = now_rfc3339()?;

    job.ok = exit_code == Some(0) && status != "failed";
    job.status = status;
    job.ended_at = Some(ended_at.clone());
    job.updated_at = ended_at;
    job.exit_code = exit_code;
    job.completed_count = terminal
        .as_ref()
        .and_then(|value| value.get("completed_count"))
        .and_then(|value| value.as_u64())
        .map(|value| value as usize)
        .unwrap_or(0);
    job.terminal = terminal;
    write_execution_job(&job)
}

fn fail_execution_job(metadata_path: PathBuf, message: String) -> Result<()> {
    let raw = fs::read_to_string(&metadata_path)
        .with_context(|| format!("failed to read {}", metadata_path.display()))?;
    let mut job: ExecutionJob = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", metadata_path.display()))?;
    let ended_at = now_rfc3339()?;
    job.ok = false;
    job.status = "failed".to_string();
    job.ended_at = Some(ended_at.clone());
    job.updated_at = ended_at;
    job.message = Some(message);
    write_execution_job(&job)
}

fn parse_runner_terminal(stdout: &str) -> Option<serde_json::Value> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }

    serde_json::from_str(trimmed).ok().or_else(|| {
        trimmed
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str(line.trim()).ok())
    })
}

fn execution_terminal_status(
    exit_code: Option<i32>,
    terminal: Option<&serde_json::Value>,
) -> String {
    let terminal_status = terminal
        .and_then(|value| value.get("status").or_else(|| value.get("error")))
        .and_then(|value| value.as_str());

    match terminal_status {
        Some("queue_clear") => "queue_clear".to_string(),
        Some("stalled") => "stalled".to_string(),
        Some("escalated") => "escalated".to_string(),
        Some("queue_validation_failed") => "queue_validation_failed".to_string(),
        Some(status) if exit_code == Some(0) => status.to_string(),
        _ if exit_code == Some(0) => "queue_clear".to_string(),
        _ => "failed".to_string(),
    }
}

fn runner_script_path() -> Result<PathBuf> {
    for key in ["CLAUDE_PLUGIN_ROOT", "MUTAGEN_PLUGIN_ROOT"] {
        if let Ok(root) = std::env::var(key) {
            let path = PathBuf::from(root).join("scripts/run_execute_next.sh");
            if path.exists() {
                return Ok(path);
            }
        }
    }

    let current_dir = std::env::current_dir().context("failed to read current directory")?;
    for base in current_dir.ancestors() {
        let repo_path = base.join("plugins/mutagen/scripts/run_execute_next.sh");
        if repo_path.exists() {
            return Ok(repo_path);
        }
    }

    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    if let Some(plugin_root) = exe.parent().and_then(|path| path.parent()) {
        let path = plugin_root.join("scripts/run_execute_next.sh");
        if path.exists() {
            return Ok(path);
        }
    }

    bail!("could not find plugins/mutagen/scripts/run_execute_next.sh")
}

fn host_name(host: HostKind) -> &'static str {
    match host {
        HostKind::Stub => "stub",
        HostKind::Codex => "codex",
        HostKind::Claude => "claude",
    }
}

fn unix_millis() -> Result<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX epoch")?
        .as_millis())
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
    let execution_jobs = read_execution_jobs(workspace_root).unwrap_or_default();
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
        let host = entry
            .get("host")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let title = match host.as_str() {
            Some(host) => format!(
                "{} {} via {}",
                entry
                    .get("slice_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("slice"),
                entry
                    .get("status")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown"),
                host
            ),
            None => format!(
                "{} {}",
                entry
                    .get("slice_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("slice"),
                entry
                    .get("status")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown")
            ),
        };
        items.push(json!({
            "kind": "dispatch",
            "timestamp": entry.get("completed_at").cloned().unwrap_or(serde_json::Value::Null),
            "title": title,
            "host": host,
            "detail": entry,
        }));
    }

    if active_state_path.exists() {
        let raw = fs::read_to_string(&active_state_path)
            .with_context(|| format!("failed to read {}", active_state_path.display()))?;
        let parsed: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", active_state_path.display()))?;
        let host = parsed
            .get("host")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let title = match host.as_str() {
            Some(host) => format!(
                "{} active at {} via {}",
                parsed
                    .get("slice_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("slice"),
                parsed
                    .get("stage")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown"),
                host,
            ),
            None => format!(
                "{} active at {}",
                parsed
                    .get("slice_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("slice"),
                parsed
                    .get("stage")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown"),
            ),
        };
        items.push(json!({
            "kind": "active",
            "timestamp": parsed.get("started_at_unix_ms").cloned().unwrap_or(serde_json::Value::Null),
            "title": title,
            "host": host,
            "detail": parsed,
        }));
    }

    for job in execution_jobs.into_iter().take(limit) {
        let timestamp = job
            .ended_at
            .clone()
            .unwrap_or_else(|| job.started_at.clone());
        let title = format!("execution {} via {}", job.status, host_name(job.host));
        items.push(json!({
            "kind": "execution",
            "timestamp": timestamp,
            "title": title,
            "host": job.host,
            "detail": job,
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

fn system_time_rfc3339(value: SystemTime) -> Result<String> {
    OffsetDateTime::from(value)
        .format(&Rfc3339)
        .context("failed to format timestamp")
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

      .mode-tabs {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 4px;
        background: rgba(255, 255, 255, 0.62);
      }

      .mode-tab {
        min-width: 0;
        border-radius: 6px;
        padding: 9px 12px;
        background: transparent;
        color: var(--ink);
      }

      .mode-tab.active {
        background: var(--accent);
        color: white;
      }

      .grid {
        display: grid;
        gap: 16px;
        grid-template-columns: minmax(0, 1.3fr) minmax(360px, 0.9fr);
      }

      .chat-layout {
        display: grid;
        gap: 16px;
      }

      .chat-shell {
        min-height: min(72vh, 820px);
        display: grid;
        grid-template-rows: auto minmax(360px, 1fr) auto;
      }

      .chat-transcript {
        display: grid;
        align-content: start;
        gap: 12px;
        overflow: auto;
        padding: 6px 2px 14px;
      }

      .chat-status {
        display: flex;
        align-items: center;
        gap: 10px;
        border: 1px solid rgba(15, 118, 110, 0.24);
        border-radius: 8px;
        padding: 10px 12px;
        margin-bottom: 12px;
        background: var(--accent-soft);
        color: var(--accent-strong);
      }

      .run-dot {
        width: 10px;
        height: 10px;
        border-radius: 999px;
        background: var(--accent);
        animation: pulse 1s ease-in-out infinite;
      }

      @keyframes pulse {
        0%, 100% { transform: scale(0.78); opacity: 0.5; }
        50% { transform: scale(1); opacity: 1; }
      }

      .chat-message {
        width: min(780px, 92%);
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 12px 14px;
        background: rgba(255, 255, 255, 0.62);
      }

      .chat-message.user {
        justify-self: end;
        background: var(--accent-soft);
        border-color: rgba(15, 118, 110, 0.28);
      }

      .chat-message.assistant {
        justify-self: start;
      }

      .chat-message.pending {
        border-color: rgba(15, 118, 110, 0.3);
        background: rgba(255, 255, 255, 0.74);
      }

      .chat-action-summary {
        display: grid;
        gap: 8px;
        margin-top: 10px;
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 10px;
        background: rgba(255, 255, 255, 0.5);
      }

      .chat-action-summary ul {
        margin: 0;
        padding-left: 18px;
      }

      .chat-action-summary li {
        margin: 4px 0;
      }

      .chat-action-summary details {
        margin-top: 2px;
      }

      .chat-action-summary summary {
        color: var(--muted);
        cursor: pointer;
        font-size: 0.86rem;
      }

      .chat-message pre {
        margin-top: 10px;
        max-height: 280px;
      }

      .chat-compose {
        display: grid;
        gap: 10px;
        border-top: 1px solid var(--line);
        padding-top: 14px;
      }

      .chat-compose textarea {
        min-height: 110px;
      }

      .quick-commands {
        display: flex;
        gap: 8px;
        flex-wrap: wrap;
      }

      .quick-commands button {
        min-width: 0;
        padding: 8px 10px;
        font-size: 0.88rem;
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

      .design-workbench {
        display: grid;
        gap: 12px;
        grid-template-columns: minmax(240px, 0.38fr) minmax(0, 1fr);
      }

      .design-doc-list {
        display: grid;
        gap: 8px;
        align-content: start;
      }

      .design-doc-tab {
        width: 100%;
        display: grid;
        gap: 4px;
        border: 1px solid var(--line);
        border-radius: 8px;
        padding: 10px;
        background: rgba(255, 255, 255, 0.54);
        color: var(--ink);
        text-align: left;
        cursor: pointer;
      }

      .design-doc-tab.selected {
        border-color: rgba(15, 118, 110, 0.45);
        background: var(--accent-soft);
      }

      .design-editor textarea {
        min-height: 420px;
        font-family: "IBM Plex Mono", "Cascadia Code", monospace;
        font-size: 0.9rem;
        line-height: 1.45;
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

      input, textarea, select {
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

      .checkbox-row {
        display: flex;
        align-items: center;
        gap: 10px;
      }

      .checkbox-row input {
        width: auto;
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

      button:disabled {
        cursor: not-allowed;
        opacity: 0.52;
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
        .design-workbench { grid-template-columns: 1fr; }
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
            <p>One surface for project direction, natural-language request intake, execution progress, and the next move without rummaging through queue JSON.</p>
          </div>
          <div class="hero-actions">
            <div class="mode-tabs" role="tablist" aria-label="Dashboard modes">
              <button id="show-control-tab" class="mode-tab active" type="button">Control Plane</button>
              <button id="show-chat-tab" class="mode-tab" type="button">Terminal</button>
            </div>
            <button id="refresh" class="secondary" type="button">Refresh Snapshot</button>
          </div>
        </div>
      </section>

      <section id="chat-tab" class="chat-layout dashboard-tab hidden">
        <article class="panel chat-shell">
          <div class="panel-header">
            <div>
              <h2>System terminal</h2>
              <p>Run workspace shell commands directly. Use the buttons to load harness-aware Codex, Claude, and Mutagen commands, then edit before running.</p>
            </div>
          </div>
          <div id="harness-chat-status" class="chat-status hidden" role="status" aria-live="polite"></div>
          <div id="harness-chat-transcript" class="chat-transcript"></div>
          <form id="harness-chat-form" class="chat-compose">
            <div class="quick-commands">
              <button type="button" class="secondary" data-chat-command="$MUTAGEN_HARNESS_BIN project status">Harness Status</button>
              <button type="button" class="secondary" data-chat-command="$MUTAGEN_HARNESS_BIN project doctor">Doctor</button>
              <button type="button" class="secondary" data-chat-command="$MUTAGEN_HARNESS_BIN project verify-generated">Verify</button>
              <button type="button" class="secondary" data-chat-command="$MUTAGEN_RUN_EXECUTE_NEXT --workspace-root $MUTAGEN_WORKSPACE_ROOT --host codex">Run Codex Harness</button>
              <button type="button" class="secondary" data-chat-command="codex exec &quot;You are operating the Mutagen harness. Use $MUTAGEN_HARNESS_BIN and $MUTAGEN_WORKSPACE_ROOT. Start by checking project status, then propose the next harness action.&quot;">Codex Prompt</button>
              <button type="button" class="secondary" data-chat-command="claude -p &quot;You are operating the Mutagen harness. Use $MUTAGEN_HARNESS_BIN and $MUTAGEN_WORKSPACE_ROOT. Start by checking project status, then propose the next harness action.&quot;">Claude Prompt</button>
            </div>
            <textarea id="harness-chat-input" placeholder="Type a shell command. Examples: $MUTAGEN_HARNESS_BIN project status, $MUTAGEN_HARNESS_BIN project verify-generated, codex exec &quot;Use the Mutagen harness in this workspace&quot;"></textarea>
            <div class="actions">
              <button id="harness-chat-send" type="submit">Run Command</button>
              <button id="harness-chat-refresh" class="secondary" type="button">Refresh Terminal</button>
            </div>
          </form>
        </article>
      </section>

      <section id="control-tab" class="grid dashboard-tab">
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
              <h2>Project setup</h2>
              <p>Create the project capsule, apply a blueprint, and scaffold the app from this dashboard.</p>
            </div>
          </div>
          <div id="project-setup-panel" class="stack"></div>
        </article>

        <article class="panel">
          <div class="panel-header">
            <div>
              <h2>Inference host</h2>
              <p>Choose whether Codex or Claude is driving execution from this dashboard.</p>
            </div>
          </div>
          <div id="inference-host-panel" class="stack"></div>
        </article>

        <article class="panel">
          <div class="panel-header">
            <div>
              <h2>Builder conversation</h2>
              <p>Talk through the product direction, capture decisions, and turn useful turns into queued work.</p>
            </div>
          </div>
          <form id="builder-message-form">
            <textarea id="builder-message" name="message" placeholder="Build a crew scheduling app for dispatchers. It should manage shifts, absences, overtime, and a simple live board for today's changes." required></textarea>
            <div class="actions">
              <button id="builder-note" class="secondary" type="button">Send</button>
              <button id="builder-save-direction" class="secondary" type="button">Save Direction</button>
              <button id="builder-queue-work" type="button">Queue Work</button>
            </div>
          </form>
          <div id="builder-thread-panel" class="stack"></div>
        </article>

        <article class="panel full">
          <div class="panel-header">
            <div>
              <h2>Design bundle</h2>
              <p>Review, seed, and edit the brief, PRD, ADR, DDD, ISC, and DSD before the queue starts guessing.</p>
            </div>
          </div>
          <div id="design-bundle-panel" class="stack"></div>
        </article>

        <article class="panel full">
          <div class="panel-header">
            <div>
              <h2>Build readiness</h2>
              <p>Preflight blockers and warnings before the dashboard advances executable work.</p>
            </div>
          </div>
          <div id="build-readiness-panel" class="stack"></div>
        </article>

        <article class="panel full">
          <div class="panel-header">
            <div>
              <h2>Execution console</h2>
              <p>Run, cancel, and inspect the harness loop that turns queued slices into finished work.</p>
            </div>
          </div>
          <div id="execution-console-panel" class="stack"></div>
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
              <h2>Request backlog</h2>
              <p>Natural-language requests translated into executable work, from raw intake to queued slices.</p>
            </div>
            <div id="feature-counts" class="actions"></div>
          </div>
          <table>
            <thead>
              <tr>
                <th>Request</th>
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
              <h2>Active request</h2>
              <p>The request currently holding the baton, if any.</p>
            </div>
          </div>
          <div id="active-feature-panel" class="stack"></div>
        </article>

        <article class="panel">
          <div class="panel-header">
            <div>
              <h2>Request detail</h2>
              <p>Slice-by-slice progress for whichever request you select from the backlog.</p>
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
      let selectedDesignDocId = "brief";
      let selectedExecutionJobId = null;
      let activeDashboardTab = "control";
      let harnessChatRunning = false;
      let harnessChatRunningText = "Harness is working.";

      const controlTab = document.getElementById("control-tab");
      const chatTab = document.getElementById("chat-tab");
      const showControlTab = document.getElementById("show-control-tab");
      const showChatTab = document.getElementById("show-chat-tab");
      const harnessChatStatus = document.getElementById("harness-chat-status");
      const harnessChatTranscript = document.getElementById("harness-chat-transcript");
      const harnessChatInput = document.getElementById("harness-chat-input");
      const harnessChatSend = document.getElementById("harness-chat-send");
      const harnessChatRefresh = document.getElementById("harness-chat-refresh");
      const responseLog = document.getElementById("response-log");
      const projectSummary = document.getElementById("project-summary");
      const projectSetupPanel = document.getElementById("project-setup-panel");
      const inferenceHostPanel = document.getElementById("inference-host-panel");
      const builderThreadPanel = document.getElementById("builder-thread-panel");
      const designBundlePanel = document.getElementById("design-bundle-panel");
      const buildReadinessPanel = document.getElementById("build-readiness-panel");
      const executionConsolePanel = document.getElementById("execution-console-panel");
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

      function setActiveDashboardTab(tab) {
        activeDashboardTab = tab;
        const chatActive = tab === "chat";
        chatTab.classList.toggle("hidden", !chatActive);
        controlTab.classList.toggle("hidden", chatActive);
        showChatTab.classList.toggle("active", chatActive);
        showControlTab.classList.toggle("active", !chatActive);

        if (chatActive) {
          loadHarnessChat();
          harnessChatInput.focus();
        }
      }

      function renderHarnessChat(state) {
        renderHarnessChatStatus();

        const history = state?.harnessChat || { messages: [] };
        const messages = history.messages || [];
        let visibleMessages = messages.length > 0
          ? messages
          : [{
              role: "assistant",
              content: "This is a workspace terminal. Commands run from the project root with MUTAGEN_HARNESS_BIN, MUTAGEN_RUN_EXECUTE_NEXT, and MUTAGEN_WORKSPACE_ROOT configured for you.",
              created_at: ""
            }];

        if (harnessChatRunning) {
          visibleMessages = [
            ...visibleMessages,
            {
              role: "assistant",
              content: harnessChatRunningText,
              created_at: "running",
              status: "running",
              pending: true
            }
          ];
        }

        harnessChatTranscript.innerHTML = visibleMessages.map((message) => {
          const actions = message.actions || [];
          const actionSummary = actions.length > 0
            ? actions.map(renderHarnessChatAction).join("")
            : "";
          const role = message.role || "assistant";
          const pendingClass = message.pending ? " pending" : "";
          return `
            <div class="chat-message ${escapeHtml(role)}${pendingClass}">
              <div class="row">
                <strong>${escapeHtml(role)}</strong>
                <span class="muted">${escapeHtml(message.created_at || "")}</span>
              </div>
              <p>${formatChatText(message.content || "")}</p>
              ${actionSummary}
            </div>
          `;
        }).join("");
        harnessChatTranscript.scrollTop = harnessChatTranscript.scrollHeight;
      }

      function renderHarnessChatAction(action) {
        const summary = action.summary || summarizeHarnessAction(action);
        const lines = Array.isArray(summary.lines) && summary.lines.length > 0
          ? summary.lines
          : ["The harness completed this action."];
        const raw = action.result === undefined ? action : action.result;
        return `
          <div class="chat-action-summary">
            <div class="row">
              <strong>${escapeHtml(summary.title || action.name || "Harness action")}</strong>
              ${badge(summary.status || raw?.status || "complete")}
            </div>
            <ul>
              ${lines.map((line) => `<li>${formatChatText(line)}</li>`).join("")}
            </ul>
            <details>
              <summary>Raw harness output</summary>
              <pre>${escapeHtml(JSON.stringify(raw, null, 2))}</pre>
            </details>
          </div>
        `;
      }

      function summarizeHarnessAction(action) {
        const result = action.result || {};
        if (action.name === "builder-message") {
          const artifact = result.assistant_message?.artifact || {};
          const flow = artifact.feature_flow || {};
          const enqueued = flow.enqueue_feature?.enqueued_slice_ids?.length || 0;
          const lines = [
            result.assistant_message?.content || "Recorded the builder request.",
            artifact.title ? `Request title: ${artifact.title}.` : "",
            flow.feature_id ? `Feature id: ${flow.feature_id}.` : "",
            enqueued > 0 ? `Prepared and queued ${enqueued} implementation slice(s).` : ""
          ].filter(Boolean);
          return {
            title: "Builder request",
            status: result.status || "recorded",
            lines
          };
        }

        if (action.name === "build-readiness") {
          const next = (result.checks || []).find((check) => check.status === "fail" && check.severity === "blocker");
          const lines = [
            `Readiness is ${result.status || "unknown"}.`,
            `${result.passed || 0} check(s) passing, ${result.blockers || 0} blocker(s), ${result.warnings || 0} warning(s).`,
            next ? `Next blocker: ${next.label}. ${next.detail}` : "No blocking check is currently first in line."
          ];
          return {
            title: "Build readiness",
            status: result.status || "unknown",
            lines
          };
        }

        return {
          title: action.name || "Harness action",
          status: result.status || "complete",
          lines: [`Harness action ${action.name || "unknown"} returned ${result.status || "complete"}.`]
        };
      }

      function formatChatText(value) {
        return escapeHtml(value).replaceAll("\n", "<br>");
      }

      function setHarnessChatRunning(running, label) {
        harnessChatRunning = running;
        harnessChatRunningText = label || "Harness is working.";
        harnessChatInput.disabled = running;
        harnessChatSend.disabled = running;
        harnessChatRefresh.disabled = running;
        document.querySelectorAll("[data-chat-command]").forEach((button) => {
          button.disabled = running;
        });
        renderHarnessChat(dashboardState);
      }

      function renderHarnessChatStatus() {
        if (harnessChatRunning) {
          harnessChatStatus.classList.remove("hidden");
          harnessChatStatus.innerHTML = `<span class="run-dot"></span><strong>Harness running</strong><span>${escapeHtml(harnessChatRunningText)}</span>`;
          return;
        }

        const terminalJob = dashboardState?.harnessChat?.current;
        if (terminalJob) {
          const job = terminalJob.id ? `Job ${terminalJob.id}` : "Terminal job";
          harnessChatStatus.classList.remove("hidden");
          harnessChatStatus.innerHTML = `<span class="run-dot"></span><strong>Terminal command running</strong><span>${escapeHtml(`${job}. Status: ${terminalJob.status || "running"}.`)}</span>`;
          return;
        }

        const current = dashboardState?.executionJobs?.current;
        if (current) {
          const host = current.host ? ` on ${current.host}` : "";
          const job = current.id ? `Job ${current.id}` : "Execution job";
          harnessChatStatus.classList.remove("hidden");
          harnessChatStatus.innerHTML = `<span class="run-dot"></span><strong>Harness loop running</strong><span>${escapeHtml(`${job}${host}. Status: ${current.status || "running"}.`)}</span>`;
          return;
        }

        harnessChatStatus.classList.add("hidden");
        harnessChatStatus.innerHTML = "";
      }

      function optimisticHarnessChat(message) {
        const existing = dashboardState?.harnessChat?.messages || [];
        dashboardState = {
          ...(dashboardState || {}),
          harnessChat: {
            ...(dashboardState?.harnessChat || {}),
            status: "running",
            messages: [
              ...existing,
              {
                role: "user",
                content: message,
                created_at: "sending"
              }
            ]
          }
        };
      }

      function harnessChatRunningLabel(message) {
        const lower = message.toLowerCase();
        if (lower.includes("codex")) {
          return "Starting Codex from the workspace terminal.";
        }
        if (lower.includes("claude")) {
          return "Starting Claude from the workspace terminal.";
        }
        if (lower.includes("mutagen") || lower.includes("harness")) {
          return "Running a Mutagen harness command in the workspace terminal.";
        }
        return "Running the command in the workspace terminal.";
      }

      function renderProjectSetup(state) {
        const data = state.dashboard || {};
        const blueprints = state.blueprints?.blueprints || [];
        const stackOptions = blueprints.map((blueprint) => `
          <option value="${escapeHtml(blueprint.stack)}" ${blueprint.stack === "vite-express-sqlite" ? "selected" : ""}>
            ${escapeHtml(blueprint.label || blueprint.stack)}
          </option>
        `).join("");

        if (data.status !== "uninitialized") {
          projectSetupPanel.innerHTML = `
            <div class="callout">
              <strong>${escapeHtml(data.project?.stack || "Project ready")}</strong>
              <div class="muted">${escapeHtml(data.workspace_root || "")}</div>
              <div class="kv">
                <div class="kv-item"><span class="muted">Capsule</span><span>${data.project?.capsule_ok ? "ok" : "attention"}</span></div>
                <div class="kv-item"><span class="muted">Scaffold</span><span>${data.project?.scaffold_ok ? "ok" : "attention"}</span></div>
                <div class="kv-item"><span class="muted">Stack</span><span>${escapeHtml(data.project?.stack || "unknown")}</span></div>
              </div>
            </div>
          `;
          return;
        }

        projectSetupPanel.innerHTML = `
          <form id="project-create-form" class="stack">
            <label for="project-name">Project name</label>
            <input id="project-name" name="name" value="" placeholder="Crew Scheduler" required>
            <div class="mini-grid">
              <div class="stack">
                <label for="project-stack">Stack</label>
                <select id="project-stack" name="stack">${stackOptions}</select>
              </div>
              <div class="stack">
                <label for="project-design-system">Design system</label>
                <input id="project-design-system" name="design_system" value="plain-css" required>
              </div>
            </div>
            <label for="project-deploy-target">Deploy target</label>
            <input id="project-deploy-target" name="deploy_target" placeholder="cloudflare, render, local">
            <label class="checkbox-row">
              <input id="project-force" type="checkbox">
              <span>Replace existing generated scaffold files</span>
            </label>
            <div class="actions">
              <button id="project-create" type="button">Create Project</button>
            </div>
          </form>
        `;

        document.getElementById("project-create").addEventListener("click", async () => {
          const name = document.getElementById("project-name").value.trim();
          const stack = document.getElementById("project-stack").value;
          const designSystem = document.getElementById("project-design-system").value.trim();
          const deployTarget = document.getElementById("project-deploy-target").value.trim();
          const force = document.getElementById("project-force").checked;

          if (!name || !stack || !designSystem) {
            setLog({ ok: false, status: "error", message: "Project name, stack, and design system are required." });
            return;
          }

          await runAction(async () => postJson("/api/project-create", {
            name,
            stack,
            design_system: designSystem,
            deploy_target: deployTarget || null,
            force
          }));
        });
      }

      function projectCard(data, previewPlan) {
        if (data.status === "uninitialized") {
          return `
            <div class="stats">
              <div class="stat"><span class="label">Workspace</span><strong>Uninitialized</strong>${badge("attention")}</div>
              <div class="stat"><span class="label">Capsule</span><strong>Missing</strong>${escapeHtml(data.capsule_path || "")}</div>
              <div class="stat"><span class="label">Requests</span><strong>0</strong>not started</div>
              <div class="stat"><span class="label">Preview</span><strong>Unavailable</strong>No project yet</div>
            </div>
            <div class="callout">
              <strong>${escapeHtml(data.message || "Create a project to continue.")}</strong>
              <div class="muted">${escapeHtml(data.workspace_root || "")}</div>
            </div>
          `;
        }

        const preview = data.project.preview;
        const active = data.active_feature;
        const previewUrl = preview.url || (previewPlan.ok ? previewPlan.url : "");
        return `
          <div class="stats">
            <div class="stat"><span class="label">Project</span><strong>${escapeHtml(data.project.stack)}</strong>${badge(data.status)}</div>
            <div class="stat"><span class="label">Requests</span><strong>${data.feature_backlog.total}</strong>${data.feature_backlog.in_queue} in queue</div>
            <div class="stat"><span class="label">Preview</span><strong>${escapeHtml(preview.status)}</strong>${previewUrl ? `<a href="${escapeHtml(previewUrl)}" target="_blank" rel="noreferrer">${escapeHtml(previewUrl)}</a>` : "No URL"}</div>
            <div class="stat"><span class="label">Active request</span><strong>${active ? escapeHtml(active.feature.title) : "None"}</strong>${active ? badge(active.status) : "Idle"}</div>
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

      function renderBuilderThread(state) {
        const data = state.dashboard || {};
        const brief = data.project_brief || { exists: false, excerpt: "", path: "" };
        const messages = state.builderThread?.messages || [];
        const messageList = messages.length > 0
          ? `<div class="timeline">
              ${messages.map((message) => `
                <div class="timeline-item ${message.role === "assistant" ? "complete" : ""}">
                  <div class="row">
                    <strong>${escapeHtml(message.role || "message")}</strong>
                    ${badge(message.action || "note")}
                  </div>
                  <div class="muted">${escapeHtml(message.created_at || "unknown")}</div>
                  <p>${escapeHtml(message.content || "")}</p>
                </div>
              `).join("")}
            </div>`
          : `<div class="muted">No conversation turns yet.</div>`;

        builderThreadPanel.innerHTML = `
          <div class="callout">
            <strong>Current direction</strong>
            <div class="muted">${brief.exists ? escapeHtml(brief.path) : "No design brief has been captured yet."}</div>
            ${
              brief.excerpt
                ? `<p>${escapeHtml(brief.excerpt)}</p>`
                : `<div class="muted">Write the project intent in plain English and the harness will preserve it here.</div>`
            }
          </div>
          ${messageList}
        `;
      }

      function renderDesignBundle(state) {
        const bundle = state.designBundle || { ok: false, status: "unavailable", readiness: {}, documents: [] };
        const docs = bundle.documents || [];

        if (!bundle.ok) {
          designBundlePanel.innerHTML = `
            <div class="callout">
              <strong>Design bundle is waiting.</strong>
              <div class="muted">${escapeHtml(bundle.message || bundle.capsule_path || "Create the project capsule first.")}</div>
            </div>
          `;
          return;
        }

        if (docs.length === 0) {
          designBundlePanel.innerHTML = `
            <div class="callout">
              <strong>No design documents are registered.</strong>
              <div class="muted">The capsule is missing its document map, which is a bold choice from the machinery.</div>
            </div>
          `;
          return;
        }

        if (!docs.some((doc) => doc.id === selectedDesignDocId)) {
          selectedDesignDocId = docs[0].id;
        }

        const selected = docs.find((doc) => doc.id === selectedDesignDocId) || docs[0];
        const readiness = bundle.readiness || {};

        designBundlePanel.innerHTML = `
          <div class="stats">
            <div class="stat"><span class="label">Bundle</span><strong>${escapeHtml(bundle.status || "draft")}</strong>${badge(bundle.status || "draft")}</div>
            <div class="stat"><span class="label">Ready</span><strong>${readiness.ready || 0}/${readiness.total || 0}</strong>${readiness.percent || 0}%</div>
            <div class="stat"><span class="label">Draft</span><strong>${readiness.draft || 0}</strong>needs detail</div>
            <div class="stat"><span class="label">Missing</span><strong>${readiness.missing || 0}</strong>paths absent</div>
          </div>
          <div class="design-workbench">
            <div class="design-doc-list">
              ${docs.map((doc) => `
                <button type="button" class="design-doc-tab ${doc.id === selected.id ? "selected" : ""}" data-design-doc="${escapeHtml(doc.id)}">
                  <span class="row"><strong>${escapeHtml(doc.label)}</strong>${badge(doc.status || "draft")}</span>
                  <span class="muted">${escapeHtml(doc.path || "")}</span>
                  <span class="muted">${doc.word_count || 0}/${doc.min_words || 0} words</span>
                </button>
              `).join("")}
            </div>
            <div class="stack design-editor">
              <div class="callout">
                <div class="row">
                  <strong>${escapeHtml(selected.label || selected.id)}</strong>
                  ${badge(selected.status || "draft")}
                </div>
                <div class="muted">${escapeHtml(selected.purpose || "")}</div>
                <div class="kv">
                  <div class="kv-item"><span class="muted">Path</span><span>${escapeHtml(selected.path || "")}</span></div>
                  <div class="kv-item"><span class="muted">Updated</span><span>${escapeHtml(selected.updated_at || "not written yet")}</span></div>
                  <div class="kv-item"><span class="muted">Size</span><span>${selected.byte_count || 0} bytes</span></div>
                </div>
              </div>
              <textarea id="design-doc-content" spellcheck="false">${escapeHtml(selected.content || "")}</textarea>
              <div class="actions">
                <button type="button" id="design-save-doc">Save Document</button>
                <button type="button" class="secondary" id="design-seed-doc">Seed Draft</button>
                <button type="button" class="secondary" id="design-refresh-bundle">Refresh Bundle</button>
              </div>
              ${
                selected.excerpt
                  ? `<div class="callout"><strong>Excerpt</strong><div class="muted">${escapeHtml(selected.excerpt)}</div></div>`
                  : `<div class="muted">This document has no meaningful body text yet.</div>`
              }
            </div>
          </div>
        `;

        designBundlePanel.querySelectorAll("button[data-design-doc]").forEach((button) => {
          button.addEventListener("click", () => {
            selectedDesignDocId = button.dataset.designDoc;
            renderDesignBundle(dashboardState);
          });
        });

        document.getElementById("design-save-doc").addEventListener("click", async () => {
          const content = document.getElementById("design-doc-content").value;
          await runAction(async () => postJson("/api/design-doc", {
            document: selectedDesignDocId,
            content
          }));
        });

        document.getElementById("design-seed-doc").addEventListener("click", async () => {
          await runAction(async () => postJson("/api/design-doc-seed", {
            document: selectedDesignDocId
          }));
        });

        document.getElementById("design-refresh-bundle").addEventListener("click", async () => {
          await loadDesignBundle();
        });
      }

      function executionIsReady(state) {
        return state?.buildReadiness?.can_execute === true;
      }

      function renderBuildReadiness(state) {
        const readiness = state.buildReadiness || { ok: false, status: "unknown", checks: [] };
        const checks = readiness.checks || [];
        const blockers = checks.filter((check) => check.status === "fail" && check.severity === "blocker");
        const warnings = checks.filter((check) => check.status === "warning");
        const canExecute = executionIsReady(state);

        if (readiness.status === "uninitialized") {
          buildReadinessPanel.innerHTML = `
            <div class="callout">
              <strong>Execution is waiting for setup.</strong>
              <div class="muted">${escapeHtml(readiness.capsule_path || "Create the project capsule first.")}</div>
            </div>
          `;
          return;
        }

        buildReadinessPanel.innerHTML = `
          <div class="stats">
            <div class="stat"><span class="label">Preflight</span><strong>${escapeHtml(readiness.status || "unknown")}</strong>${badge(readiness.status || "unknown")}</div>
            <div class="stat"><span class="label">Can execute</span><strong>${canExecute ? "Yes" : "No"}</strong>${canExecute ? "gate open" : "blocked"}</div>
            <div class="stat"><span class="label">Blockers</span><strong>${readiness.blockers || 0}</strong>must fix</div>
            <div class="stat"><span class="label">Warnings</span><strong>${readiness.warnings || 0}</strong>can proceed</div>
          </div>
          <div class="mini-grid">
            <div class="callout">
              <strong>Execution gate</strong>
              ${
                blockers.length === 0
                  ? `<div class="muted">No blocking checks are failing.</div>`
                  : `<div class="timeline">
                      ${blockers.map((check) => `
                        <div class="timeline-item blocked">
                          <div class="row"><strong>${escapeHtml(check.label)}</strong>${badge(check.category || "check")}</div>
                          <div class="muted">${escapeHtml(check.detail || "")}</div>
                          <div class="muted">Next: ${escapeHtml(check.action || "Fix check")}</div>
                        </div>
                      `).join("")}
                    </div>`
              }
            </div>
            <div class="callout">
              <strong>Warnings</strong>
              ${
                warnings.length === 0
                  ? `<div class="muted">No warnings. Unsettlingly tidy.</div>`
                  : `<div class="timeline">
                      ${warnings.map((check) => `
                        <div class="timeline-item">
                          <div class="row"><strong>${escapeHtml(check.label)}</strong>${badge(check.category || "check")}</div>
                          <div class="muted">${escapeHtml(check.detail || "")}</div>
                          <div class="muted">Next: ${escapeHtml(check.action || "Review warning")}</div>
                        </div>
                      `).join("")}
                    </div>`
              }
            </div>
          </div>
          <div class="callout">
            <strong>All checks</strong>
            <div class="actions">
              <button type="button" class="secondary" id="readiness-refresh">Refresh Gate</button>
              <button type="button" class="secondary" id="readiness-run-verify">Run Verify Generated</button>
              <button type="button" id="readiness-fix-next">Fix Next Blocker</button>
              <button type="button" class="secondary" id="readiness-run-safe-repairs">Run Safe Repairs</button>
            </div>
            <div class="kv">
              ${checks.map((check, index) => `
                <div class="kv-item">
                  <span>${escapeHtml(check.label || check.id)}<br><span class="muted">${escapeHtml(check.detail || "")}</span></span>
                  <span>${badge(check.status || "unknown")}</span>
                  <button type="button" class="secondary" data-repair-check="${index}">${escapeHtml(check.action || "Review")}</button>
                </div>
              `).join("")}
            </div>
          </div>
        `;

        document.getElementById("readiness-refresh").addEventListener("click", async () => {
          await loadBuildReadiness();
        });

        document.getElementById("readiness-run-verify").addEventListener("click", async () => {
          await runAction(async () => postJson("/api/verify-generated", {}));
        });

        document.getElementById("readiness-fix-next").addEventListener("click", async () => {
          const next = blockers[0] || warnings[0];
          if (!next) {
            setLog({ ok: true, status: "ready", message: "No readiness blockers need repair." });
            return;
          }
          await runRepairAction(next);
        });

        document.getElementById("readiness-run-safe-repairs").addEventListener("click", async () => {
          await runSafeRepairs(checks);
        });

        buildReadinessPanel.querySelectorAll("[data-repair-check]").forEach((button) => {
          button.addEventListener("click", async () => {
            const check = checks[Number(button.dataset.repairCheck)];
            await runRepairAction(check);
          });
        });
      }

      function renderExecutionConsole(state) {
        const jobsState = state.executionJobs || { jobs: [], current: null };
        const jobs = jobsState.jobs || [];
        const selectedJob = state.executionJob?.job || jobs.find((job) => job.id === selectedExecutionJobId) || jobsState.current || jobs[0] || null;
        const runningJob = jobsState.current || jobs.find((job) => job.status === "running") || null;
        const canRun = executionIsReady(state) && !runningJob;
        const stdoutLines = state.executionJob?.stdout_lines || [];
        const stderrLines = state.executionJob?.stderr_lines || [];

        if (selectedJob && selectedExecutionJobId !== selectedJob.id) {
          selectedExecutionJobId = selectedJob.id;
        }

        executionConsolePanel.innerHTML = `
          <div class="mini-grid">
            <div class="callout">
              <strong>Harness loop</strong>
              <div class="kv">
                <div class="kv-item"><span class="muted">Selected job</span><span>${escapeHtml(selectedJob?.id || "none")}</span></div>
                <div class="kv-item"><span class="muted">Status</span><span>${selectedJob ? badge(selectedJob.status || "unknown") : "No runs yet"}</span></div>
                <div class="kv-item"><span class="muted">Host</span><span>${escapeHtml(hostLabel(selectedJob?.host || state.inferenceHost?.selected_host || ""))}</span></div>
                <div class="kv-item"><span class="muted">Started</span><span>${escapeHtml(selectedJob?.started_at || "not started")}</span></div>
                <div class="kv-item"><span class="muted">Ended</span><span>${escapeHtml(selectedJob?.ended_at || "running or not started")}</span></div>
                <div class="kv-item"><span class="muted">Completed</span><span>${selectedJob?.completed_count ?? 0}</span></div>
              </div>
              <div class="actions">
                <button type="button" id="execution-run" ${canRun ? "" : "disabled"}>Run Harness Loop</button>
                <button type="button" class="secondary" id="execution-cancel" ${runningJob ? "" : "disabled"}>Cancel Run</button>
                <button type="button" class="secondary" id="execution-refresh">Refresh Run</button>
                <button type="button" class="secondary" id="execution-open-active">Open Active Slice</button>
              </div>
            </div>
            <div class="callout">
              <strong>Recent runs</strong>
              ${
                jobs.length === 0
                  ? `<div class="muted">No execution jobs have been launched yet.</div>`
                  : `<div class="timeline">
                      ${jobs.map((job) => `
                        <div class="timeline-item ${job.status === "running" ? "active" : job.ok ? "complete" : "blocked"}" data-execution-job="${escapeHtml(job.id)}">
                          <div class="row"><strong>${escapeHtml(job.id)}</strong>${badge(job.status || "unknown")}</div>
                          <div class="muted">${escapeHtml(hostLabel(job.host))} · ${escapeHtml(job.updated_at || job.started_at || "")}</div>
                        </div>
                      `).join("")}
                    </div>`
              }
            </div>
          </div>
          <div class="mini-grid">
            <div class="callout">
              <strong>Terminal payload</strong>
              ${
                selectedJob?.terminal
                  ? `<pre>${escapeHtml(JSON.stringify(selectedJob.terminal, null, 2))}</pre>`
                  : `<div class="muted">No terminal payload has been recorded yet.</div>`
              }
            </div>
            <div class="callout">
              <strong>Runner logs</strong>
              <div class="muted">${escapeHtml(selectedJob?.stdout_path || "")}</div>
              ${
                stdoutLines.length === 0 && stderrLines.length === 0
                  ? `<div class="muted">No runner output yet.</div>`
                  : `<pre>${escapeHtml([...stdoutLines, ...stderrLines.map((line) => `stderr: ${line}`)].join("\n"))}</pre>`
              }
            </div>
          </div>
        `;

        document.getElementById("execution-run").addEventListener("click", async () => {
          await runExecutionJob();
        });

        document.getElementById("execution-cancel").addEventListener("click", async () => {
          if (!runningJob) return;
          await runAction(async () => postJson("/api/execution-cancel", { id: runningJob.id }));
          await loadExecutionJobs();
        });

        document.getElementById("execution-refresh").addEventListener("click", async () => {
          await loadExecutionJobs();
        });

        document.getElementById("execution-open-active").addEventListener("click", async () => {
          const activeSliceId = dashboardState?.queueStatus?.active_slice_id || dashboardState?.dashboard?.active_feature?.active_slice?.id;
          if (!activeSliceId) {
            setLog({ ok: false, status: "no_active_slice", message: "No active slice is available to open." });
            return;
          }
          await loadSliceArtifacts(activeSliceId);
        });

        executionConsolePanel.querySelectorAll("[data-execution-job]").forEach((item) => {
          item.addEventListener("click", async () => {
            selectedExecutionJobId = item.dataset.executionJob;
            await loadExecutionJob(selectedExecutionJobId);
          });
        });
      }

      function hostLabel(host) {
        if (host === "codex") return "Codex";
        if (host === "claude") return "Claude";
        if (host === "stub") return "Stub";
        return host || "Unknown";
      }

      function renderInferenceHost(state) {
        const host = state.inferenceHost || {
          selected_host: "stub",
          default_host: "stub",
          available_hosts: ["codex", "claude", "stub"],
          profile: { degraded_features: [] }
        };
        const profile = host.profile || { degraded_features: [] };
        const options = (host.available_hosts || []).map((value) => `
          <option value="${escapeHtml(value)}" ${value === host.selected_host ? "selected" : ""}>${escapeHtml(hostLabel(value))}</option>
        `).join("");
        const degraded = profile.degraded_features || [];
        const updatedAt = host.updated_at || "Using launch default";

        inferenceHostPanel.innerHTML = `
          <div class="mini-grid">
            <div class="callout">
              <strong>Active host</strong>
              <div class="kv">
                <div class="kv-item"><span class="muted">Selected</span><span>${escapeHtml(hostLabel(host.selected_host))}</span></div>
                <div class="kv-item"><span class="muted">Default</span><span>${escapeHtml(hostLabel(host.default_host))}</span></div>
                <div class="kv-item"><span class="muted">Stored</span><span>${host.persisted ? "yes" : "no"}</span></div>
                <div class="kv-item"><span class="muted">Updated</span><span>${escapeHtml(updatedAt)}</span></div>
              </div>
              <div class="actions">
                <select id="inference-host-select">${options}</select>
                <button type="button" id="apply-inference-host">Apply Host</button>
              </div>
            </div>
            <div class="callout">
              <strong>Execution profile</strong>
              <div class="kv">
                <div class="kv-item"><span class="muted">Scope</span><span>${escapeHtml(String(profile.scope_enforcement || "unknown").replaceAll("_", " "))}</span></div>
                <div class="kv-item"><span class="muted">Worktree</span><span>${escapeHtml(String(profile.worktree_isolation || "unknown").replaceAll("_", " "))}</span></div>
                <div class="kv-item"><span class="muted">Dispatch</span><span>${escapeHtml(String(profile.parallel_dispatch || "unknown").replaceAll("_", " "))}</span></div>
                <div class="kv-item"><span class="muted">Interrupts</span><span>${escapeHtml(String(profile.stage_interrupts || "unknown").replaceAll("_", " "))}</span></div>
              </div>
              ${
                degraded.length > 0
                  ? `<div class="muted">Degraded features: ${escapeHtml(degraded.join(", "))}</div>`
                  : `<div class="muted">No degraded host capabilities reported.</div>`
              }
            </div>
          </div>
        `;

        document.getElementById("apply-inference-host").addEventListener("click", async () => {
          const selectedHost = document.getElementById("inference-host-select").value;
          await runAction(async () => postJson("/api/inference-host", { host: selectedHost }));
        });
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

      async function loadDesignBundle() {
        const designBundle = await safeFetchJson("/api/design-bundle");
        dashboardState = {
          ...(dashboardState || {}),
          designBundle
        };
        renderDesignBundle(dashboardState);
        setLog(designBundle);
      }

      async function loadBuildReadiness() {
        const buildReadiness = await safeFetchJson("/api/build-readiness");
        dashboardState = {
          ...(dashboardState || {}),
          buildReadiness
        };
        renderBuildReadiness(dashboardState);
        setLog(buildReadiness);
      }

      async function refreshOperationalState() {
        const [buildReadiness, buildLog, previewLog, queueStatus, activityFeed, designBundle] = await Promise.all([
          safeFetchJson("/api/build-readiness"),
          safeFetchJson("/api/build-log?limit=12"),
          safeFetchJson("/api/preview-log?limit=80"),
          safeFetchJson("/api/queue-status"),
          safeFetchJson("/api/activity-feed?limit=16"),
          safeFetchJson("/api/design-bundle")
        ]);
        dashboardState = {
          ...(dashboardState || {}),
          buildReadiness,
          buildLog,
          previewLog,
          queueStatus,
          activityFeed,
          designBundle
        };
        renderBuildReadiness(dashboardState);
        renderDesignBundle(dashboardState);
        renderDebugDetail(dashboardState);
        renderQueueControl(dashboardState);
        renderActivityFeed(dashboardState);
      }

      async function runRepairAction(check) {
        if (!check) {
          setLog({ ok: false, status: "error", message: "No readiness check was selected." });
          return;
        }

        const payload = check.repair_payload || {};
        const action = check.repair_action || "manual";

        if (action === "manual") {
          setLog({ ok: false, status: "manual_repair_required", check });
          return;
        }

        if (action === "focus_builder_queue_work") {
          document.getElementById("builder-message").focus();
          setLog({
            ok: true,
            status: "builder_focus",
            message: "Write the next request in the builder conversation, then use Queue Work.",
            check
          });
          return;
        }

        await runAction(async () => {
          if (action === "repair_scaffold") return postJson("/api/repair-scaffold", payload);
          if (action === "run_doctor") return fetchJson("/api/doctor");
          if (action === "seed_design_bundle") return postJson("/api/design-bundle-seed", payload);
          if (action === "run_command") return postJson("/api/run-command", payload);
          if (action === "preview_start") return postJson("/api/preview-start", payload);
          if (action === "preview_check") return fetchJson("/api/preview-check");
          return Promise.resolve({ ok: false, status: "unsupported_repair_action", check });
        });
        await refreshOperationalState();
      }

      async function runSafeRepairs(checks) {
        const safeActions = new Set(["repair_scaffold", "run_doctor", "seed_design_bundle", "run_command", "preview_start", "preview_check"]);
        const repairs = checks.filter((check) =>
          (check.status === "fail" || check.status === "warning") &&
          safeActions.has(check.repair_action || "")
        );

        if (repairs.length === 0) {
          setLog({ ok: true, status: "no_safe_repairs", message: "No safe repair actions are currently available." });
          return;
        }

        const results = [];
        for (const check of repairs) {
          await runRepairAction(check);
          results.push({ id: check.id, action: check.repair_action });
        }
        setLog({ ok: true, status: "safe_repairs_finished", repairs: results });
        await refreshDashboard();
      }

      function renderQueueControl(state) {
        const queue = state.queueStatus || { selection: { status: "unknown", blocked: [] }, validation: { issues: [] } };
        const selection = queue.selection || {};
        const blocked = selection.blocked || [];
        const issues = queue.validation?.issues || [];
        const nextReady = selection.next_ready_slice;
        const canExecute = executionIsReady(state);

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
                <button type="button" id="queue-prepare-next" ${canExecute ? "" : "disabled"}>Prepare Next Ready Slice</button>
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
          await runExecutionAction(async () => postJson("/api/queue-prepare-next", {}));
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
                    ${(() => {
                      const itemHost = item.host || item.detail?.host || "";
                      const hostMeta = itemHost
                        ? `<div class="muted">Host ${escapeHtml(hostLabel(itemHost))}</div>`
                        : "";
                      return `
                    <div class="timeline-item ${item.kind === "dispatch" ? "complete" : item.kind === "active" ? "active" : ""}">
                      <div class="row">
                        <strong>${escapeHtml(item.title || item.kind || "event")}</strong>
                        ${badge(item.kind || "event")}
                      </div>
                      <div class="muted">${escapeHtml(String(item.timestamp ?? "unknown"))}</div>
                      ${hostMeta}
                      <pre>${escapeHtml(JSON.stringify(item.detail, null, 2))}</pre>
                    </div>
                      `;
                    })()}
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

      async function loadExecutionJobs() {
        const executionJobs = await safeFetchJson("/api/execution-jobs?limit=8");
        const selectedId = selectedExecutionJobId || executionJobs.current?.id || executionJobs.jobs?.[0]?.id;
        const executionJob = selectedId
          ? await safeFetchJson(`/api/execution-job?id=${encodeURIComponent(selectedId)}`)
          : null;
        dashboardState = {
          ...(dashboardState || {}),
          executionJobs,
          executionJob
        };
        renderExecutionConsole(dashboardState);
        setLog(executionJob || executionJobs);
      }

      async function loadExecutionJob(jobId) {
        const executionJob = await safeFetchJson(`/api/execution-job?id=${encodeURIComponent(jobId)}`);
        dashboardState = {
          ...(dashboardState || {}),
          executionJob
        };
        renderExecutionConsole(dashboardState);
        setLog(executionJob);
      }

      async function runExecutionJob() {
        const readiness = dashboardState?.buildReadiness || await safeFetchJson("/api/build-readiness");
        if (!readiness.can_execute) {
          setLog({
            ok: false,
            status: "preflight_blocked",
            message: "Build readiness has blockers. Clear the gate before running the harness loop.",
            readiness
          });
          return;
        }

        const payload = await postJson("/api/execution-run", {});
        selectedExecutionJobId = payload.job?.id || selectedExecutionJobId;
        setLog(payload);
        await refreshDashboard();
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
        const canExecute = executionIsReady(dashboardState);

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
            <td><button type="button" data-action="execute" data-feature-id="${feature.id}" ${canExecute ? "" : "disabled"}>Advance Request</button></td>
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
            await runExecutionAction(async () => postJson("/api/execute-feature", {
              feature_id: button.dataset.featureId
            }));
          });
        });
      }

      function renderActiveFeature(data) {
        const active = data.active_feature;
        const activeHost = active?.active_slice?.host || dashboardState?.inferenceHost?.selected_host || "";
        const canExecute = executionIsReady(dashboardState);

        if (!active) {
          activeFeaturePanel.innerHTML = `
            <div class="callout">
              <strong>Nothing is currently claimed.</strong>
              <div class="muted">Once a request slice is prepared, its live progress and active agent will show up here.</div>
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
                    <div class="kv-item"><span class="muted">Host</span><span>${escapeHtml(hostLabel(activeHost))}</span></div>
                  </div>`
                : `<div class="muted">No active slice metadata yet.</div>`
            }
            <div class="actions">
              <button type="button" id="active-execute" ${canExecute ? "" : "disabled"}>Advance Request</button>
              <button type="button" class="secondary" id="active-open-detail">Open Detail</button>
              ${active.active_slice ? `<button type="button" class="secondary" id="active-open-artifacts">Open Slice Artifacts</button>` : ""}
            </div>
          </div>
        `;

        document.getElementById("active-execute").addEventListener("click", async () => {
          await runExecutionAction(async () => postJson("/api/execute-feature", {
            feature_id: active.feature.id
          }));
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
              <strong>No request selected.</strong>
              <div class="muted">Choose a request from the backlog to inspect slice progress.</div>
            </div>
          `;
          return;
        }

        const detail = await fetchJson(`/api/feature-progress?feature_id=${encodeURIComponent(featureId)}`);
        selectedFeatureId = featureId;
        const canExecute = executionIsReady(dashboardState);
        featureDetailPanel.innerHTML = `
          <div class="feature-card">
            <div class="row">
              <strong>${escapeHtml(detail.feature.title)}</strong>
              ${badge(detail.status)}
            </div>
            <div class="muted">${escapeHtml(detail.feature.id)}</div>
            <div class="actions">
              <button type="button" id="detail-execute" ${canExecute ? "" : "disabled"}>Execute Next Slice</button>
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
          await runExecutionAction(async () => postJson("/api/execute-feature", { feature_id: detail.feature.id }));
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

      async function runExecutionAction(action) {
        const readiness = dashboardState?.buildReadiness || await safeFetchJson("/api/build-readiness");
        dashboardState = {
          ...(dashboardState || {}),
          buildReadiness: readiness
        };
        renderBuildReadiness(dashboardState);

        if (!readiness.can_execute) {
          setLog({
            ok: false,
            status: "preflight_blocked",
            message: "Build readiness has blockers. Fix the preflight gate before advancing executable work.",
            readiness
          });
          return;
        }

        await runAction(action);
      }

      function renderUninitializedWorkspace(data) {
        const message = `<div class="callout"><strong>Project setup is waiting.</strong><div class="muted">${escapeHtml(data.workspace_root || "")}</div></div>`;
        builderThreadPanel.innerHTML = message;
        designBundlePanel.innerHTML = message;
        buildReadinessPanel.innerHTML = message;
        executionConsolePanel.innerHTML = message;
        bootstrapHealthPanel.innerHTML = message;
        previewBuildPanel.innerHTML = message;
        debugDetailPanel.innerHTML = message;
        queueControlPanel.innerHTML = message;
        activityFeedPanel.innerHTML = message;
        featureCounts.innerHTML = "";
        featureTable.innerHTML = "";
        activeFeaturePanel.innerHTML = message;
        featureDetailPanel.innerHTML = message;
        sliceArtifactsPanel.innerHTML = message;
      }

      async function refreshDashboard() {
        const [data, previewPlan, inferenceHost, blueprints, builderThread, designBundle, buildReadiness, harnessChat] = await Promise.all([
          fetchJson("/api/dashboard"),
          safeFetchJson("/api/preview-plan"),
          safeFetchJson("/api/inference-host"),
          safeFetchJson("/api/project-blueprints"),
          safeFetchJson("/api/builder-thread?limit=24"),
          safeFetchJson("/api/design-bundle"),
          safeFetchJson("/api/build-readiness"),
          safeFetchJson("/api/harness-terminal?limit=80")
        ]);
        const [buildLog, previewLog, queueStatus, activityFeed, executionJobs] = await Promise.all([
          safeFetchJson("/api/build-log?limit=12"),
          safeFetchJson("/api/preview-log?limit=80"),
          safeFetchJson("/api/queue-status"),
          safeFetchJson("/api/activity-feed?limit=16"),
          safeFetchJson("/api/execution-jobs?limit=8")
        ]);
        const selectedJobId = selectedExecutionJobId || executionJobs.current?.id || executionJobs.jobs?.[0]?.id;
        const executionJob = selectedJobId
          ? await safeFetchJson(`/api/execution-job?id=${encodeURIComponent(selectedJobId)}`)
          : null;
        const chatState = harnessChatRunning && dashboardState?.harnessChat
          ? dashboardState.harnessChat
          : harnessChat;
        dashboardState = { dashboard: data, previewPlan, inferenceHost, blueprints, builderThread, designBundle, buildReadiness, harnessChat: chatState, buildLog, previewLog, queueStatus, activityFeed, executionJobs, executionJob };
        renderHarnessChat(dashboardState);
        projectSummary.innerHTML = projectCard(data, previewPlan);
        renderProjectSetup(dashboardState);
        renderInferenceHost(dashboardState);

        if (data.status === "uninitialized") {
          renderUninitializedWorkspace(data);
          return;
        }

        renderBuilderThread(dashboardState);
        renderDesignBundle(dashboardState);
        renderBuildReadiness(dashboardState);
        renderExecutionConsole(dashboardState);
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

      async function submitBuilderMessage(action) {
        const message = document.getElementById("builder-message").value.trim();
        if (!message) {
          setLog({ ok: false, status: "error", message: "Builder message is required" });
          return;
        }

        try {
          const payload = await postJson("/api/builder-message", {
            message,
            action
          });
          setLog(payload);
          if (payload.ok) {
            document.getElementById("builder-message").value = "";
          }
          await refreshDashboard();
        } catch (error) {
          setLog({ ok: false, status: "error", message: error.message });
        }
      }

      async function loadHarnessChat() {
        const harnessChat = await safeFetchJson("/api/harness-terminal?limit=80");
        dashboardState = {
          ...(dashboardState || {}),
          harnessChat
        };
        renderHarnessChat(dashboardState);
      }

      async function sendHarnessChat(message) {
        const trimmed = message.trim();
        if (!trimmed) {
          setLog({ ok: false, status: "error", message: "Terminal command is required." });
          return;
        }

        harnessChatInput.value = "";
        optimisticHarnessChat(trimmed);
        setHarnessChatRunning(true, harnessChatRunningLabel(trimmed));
        try {
          const payload = await postJson("/api/harness-terminal", { command: trimmed });
          setLog(payload);
          dashboardState = {
            ...(dashboardState || {}),
            harnessChat: payload
          };
          setHarnessChatRunning(false);
          renderHarnessChat(dashboardState);
          await refreshDashboard();
        } catch (error) {
          setHarnessChatRunning(false);
          throw error;
        }
      }

      document.getElementById("refresh").addEventListener("click", refreshDashboard);
      showControlTab.addEventListener("click", () => setActiveDashboardTab("control"));
      showChatTab.addEventListener("click", () => setActiveDashboardTab("chat"));
      document.getElementById("harness-chat-form").addEventListener("submit", async (event) => {
        event.preventDefault();
        try {
          await sendHarnessChat(harnessChatInput.value);
        } catch (error) {
          setLog({ ok: false, status: "error", message: error.message });
          await loadHarnessChat();
        }
      });
      document.getElementById("harness-chat-refresh").addEventListener("click", async () => {
        await loadHarnessChat();
      });
      document.querySelectorAll("[data-chat-command]").forEach((button) => {
        button.addEventListener("click", () => {
          harnessChatInput.value = button.dataset.chatCommand;
          harnessChatInput.focus();
        });
      });
      document.getElementById("builder-note").addEventListener("click", async () => {
        await submitBuilderMessage("note");
      });
      document.getElementById("builder-save-direction").addEventListener("click", async () => {
        await submitBuilderMessage("save_direction");
      });
      document.getElementById("builder-queue-work").addEventListener("click", async () => {
        await submitBuilderMessage("queue_work");
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
        ProjectAddFeatureOptions, ProjectCreateOptions, ProjectFeatureFlowOptions, add_feature,
        create_project, feature_flow,
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
        assert!(body.contains("/api/design-bundle"));
        assert!(body.contains("/api/design-bundle-seed"));
        assert!(body.contains("/api/build-readiness"));
        assert!(body.contains("/api/execution-run"));
        assert!(body.contains("/api/harness-terminal"));
        assert!(body.contains("Terminal"));
        assert!(body.contains("System terminal"));
        assert!(body.contains("Terminal command running"));
        assert!(body.contains("Raw harness output"));
        assert!(body.contains("Type a shell command"));
        assert!(body.contains("Project setup"));
        assert!(body.contains("Inference host"));
        assert!(body.contains("Builder conversation"));
        assert!(body.contains("Design bundle"));
        assert!(body.contains("Build readiness"));
        assert!(body.contains("Fix Next Blocker"));
        assert!(body.contains("Run Safe Repairs"));
        assert!(body.contains("Execution console"));
        assert!(body.contains("Run Harness Loop"));
        assert!(body.contains("Queue Work"));
        assert!(body.contains("Save Document"));
    }

    #[test]
    fn route_dashboard_api_reports_uninitialized_workspace() {
        let workspace = TestWorkspace::new("dashboard-uninitialized");

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
        assert!(body.contains("\"status\": \"uninitialized\""));
        assert!(body.contains(".mutagen/project.json"));
    }

    #[test]
    fn route_dashboard_blueprints_and_project_create_initialize_workspace() {
        let workspace = TestWorkspace::new("dashboard-project-create");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/project-blueprints",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("blueprints should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"stack\": \"nextjs-postgres\""));
        assert!(body.contains("\"stack\": \"vite-express-sqlite\""));
        assert!(body.contains("\"stack\": \"fastapi-react\""));
        assert!(body.contains("\"stack\": \"aspnet-blazor\""));
        assert!(body.contains("\"stack\": \"cloudflare-worker\""));
        assert!(body.contains("\"stack\": \"rust-bevy\""));

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/project-create",
            br#"{"name":"Crew Scheduler","stack":"vite-express-sqlite","design_system":"plain-css"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("project create should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"created\""));
        assert!(workspace.root.join(".mutagen/project.json").exists());
        assert!(workspace.root.join("src/App.jsx").exists());

        let (_, _, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/dashboard",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("dashboard should render after create");

        assert!(!body.contains("\"status\": \"uninitialized\""));
        assert!(body.contains("\"stack\": \"vite-express-sqlite\""));
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
    fn route_dashboard_project_intake_updates_brief_and_can_queue_work() {
        let workspace = TestWorkspace::new("dashboard-project-intake");

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
            "/api/project-intake",
            br#"{"prompt":"Build a crew scheduling app for dispatchers. It should manage shifts, absences, and overtime.","queue_feature":true}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("project intake should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"brief_updated_and_feature_flow_ready\""));
        assert!(body.contains("\"feature_flow\""));

        let brief = fs::read_to_string(workspace.root.join(".mutagen/design/brief.md"))
            .expect("design brief should read");
        assert!(brief.contains("## Current direction"));
        assert!(brief.contains("Build a crew scheduling app for dispatchers."));

        let queue = fs::read_to_string(workspace.root.join("slices/queue.json"))
            .expect("queue should read");
        assert!(queue.contains("\"slices\""));
        assert!(queue.contains("Build a crew scheduling app for dispatchers"));
    }

    #[test]
    fn route_dashboard_builder_thread_records_notes() {
        let workspace = TestWorkspace::new("dashboard-builder-note");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/builder-message",
            br#"{"message":"Remember that dispatchers care about overtime visibility.","action":"note"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("builder message should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"recorded\""));
        assert!(body.contains("overtime visibility"));

        let thread = fs::read_to_string(workspace.root.join(".mutagen/state/builder-thread.jsonl"))
            .expect("builder thread should read");
        assert_eq!(thread.lines().count(), 2);
        assert!(thread.contains("\"role\":\"user\""));
        assert!(thread.contains("\"role\":\"assistant\""));
    }

    #[test]
    fn route_dashboard_builder_message_updates_brief_and_can_queue_work() {
        let workspace = TestWorkspace::new("dashboard-builder-message");

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
            "/api/builder-message",
            br#"{"message":"Build a crew scheduling app for dispatchers with shift trades and absence tracking.","action":"queue_work"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("builder message should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"brief_updated_and_feature_flow_ready\""));
        assert!(body.contains("\"action\": \"queue_work\""));
        assert!(body.contains("\"feature_flow\""));

        let brief = fs::read_to_string(workspace.root.join(".mutagen/design/brief.md"))
            .expect("design brief should read");
        assert!(brief.contains("Build a crew scheduling app for dispatchers"));

        let thread = fs::read_to_string(workspace.root.join(".mutagen/state/builder-thread.jsonl"))
            .expect("builder thread should read");
        assert_eq!(thread.lines().count(), 2);
        assert!(thread.contains("queued"));

        let queue = fs::read_to_string(workspace.root.join("slices/queue.json"))
            .expect("queue should read");
        assert!(queue.contains("shift trades"));
    }

    #[test]
    fn route_dashboard_builder_thread_returns_recent_messages() {
        let workspace = TestWorkspace::new("dashboard-builder-thread");

        route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/builder-message",
            br#"{"message":"First note.","action":"note"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("builder note should render");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/builder-thread?limit=1",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("builder thread should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"messages\""));
        assert!(body.contains("Noted. I kept that in the builder thread."));
        assert!(!body.contains("First note."));
    }

    #[test]
    fn route_dashboard_harness_terminal_runs_workspace_command() {
        let workspace = TestWorkspace::new("dashboard-harness-terminal");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/harness-terminal",
            br#"{"command":"printf terminal-smoke"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("terminal command should start");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"started\""));
        assert!(body.contains("\"terminal-command\""));
        assert!(body.contains("printf terminal-smoke"));

        std::thread::sleep(std::time::Duration::from_millis(120));

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/harness-terminal?limit=5",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("terminal history should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("terminal-smoke"));
        assert!(body.contains("Terminal command"));
    }

    #[test]
    fn route_dashboard_harness_chat_lists_blueprints_and_persists_messages() {
        let workspace = TestWorkspace::new("dashboard-harness-chat-blueprints");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/harness-chat",
            br#"{"message":"/blueprints"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("harness chat should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"blueprints\""));
        assert!(body.contains("\"role\": \"user\""));
        assert!(body.contains("\"role\": \"assistant\""));
        assert!(body.contains("\"stack\": \"nextjs-postgres\""));
        assert!(body.contains("\"stack\": \"rust-bevy\""));

        let (status, content_type, history) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/harness-chat?limit=10",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("harness chat history should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(history.contains("/blueprints"));
        assert!(history.contains("Here are the stacks"));
    }

    #[test]
    fn route_dashboard_harness_chat_queue_command_uses_builder_flow() {
        let workspace = TestWorkspace::new("dashboard-harness-chat-queue");

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
            "/api/harness-chat",
            br#"{"message":"/queue Add shift trade approvals"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("harness chat queue should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("saved that as project direction"));
        assert!(body.contains("\"builder-message\""));
        assert!(body.contains("\"summary\""));
        assert!(body.contains("Builder request"));
        assert!(body.contains("Prepared and queued"));

        let queue = fs::read_to_string(workspace.root.join("slices/queue.json"))
            .expect("queue should read");
        assert!(queue.contains("shift trade approvals"));

        let builder_thread =
            fs::read_to_string(workspace.root.join(".mutagen/state/builder-thread.jsonl"))
                .expect("builder thread should read");
        assert!(builder_thread.contains("queue_work"));
    }

    #[test]
    fn route_dashboard_harness_chat_seed_design_bundle_command() {
        let workspace = TestWorkspace::new("dashboard-harness-chat-seed");

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
            "/api/harness-chat",
            br#"{"message":"/seed-design"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("harness chat seed should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"design_seeded\""));
        assert!(body.contains("\"design-bundle-seed\""));
        assert!(body.contains("\"ready\": 6"));
    }

    #[test]
    fn route_dashboard_harness_chat_accepts_natural_project_create_and_host() {
        let workspace = TestWorkspace::new("dashboard-harness-chat-natural-create");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/harness-chat",
            br#"{"message":"create project called Tiny Planet using rust-bevy"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("harness chat create should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("Created `Tiny Planet`"));
        assert!(workspace.root.join("Cargo.toml").exists());

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/harness-chat",
            br#"{"message":"set host to codex"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("harness chat host should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"host_updated\""));
        assert!(body.contains("\"selected_host\": \"codex\""));
    }

    #[test]
    fn route_dashboard_harness_chat_status_always_writes_a_response() {
        let workspace = TestWorkspace::new("dashboard-harness-chat-status-error");
        fs::create_dir_all(workspace.root.join(".mutagen")).expect("mutagen dir should exist");
        fs::write(workspace.root.join(".mutagen/project.json"), "{not-json")
            .expect("bad capsule should be written");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/harness-chat",
            br#"{"message":"/status"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("harness chat status should still render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"action_failed\""));
        assert!(body.contains("I tried to run that through the harness"));
        assert!(body.contains("\"role\": \"assistant\""));

        let chat = fs::read_to_string(workspace.root.join(".mutagen/state/dashboard-chat.jsonl"))
            .expect("chat history should read");
        assert!(chat.contains("/status"));
        assert!(chat.contains("action_failed"));
    }

    #[test]
    fn route_dashboard_harness_chat_status_reports_missing_preview_command() {
        let workspace = TestWorkspace::new("dashboard-harness-chat-status-preview-command");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        let capsule_path = workspace.root.join(".mutagen/project.json");
        let mut capsule: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&capsule_path).expect("capsule should read"))
                .expect("capsule should parse");
        capsule["commands"]["dev"] = serde_json::Value::String(String::new());
        fs::write(
            &capsule_path,
            serde_json::to_string_pretty(&capsule).expect("capsule should serialize"),
        )
        .expect("capsule should write");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/harness-chat",
            br#"{"message":"/status"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("harness chat status should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"blocked\""));
        assert!(body.contains("project command `dev` is not configured"));
        assert!(!body.contains("\"status\": \"action_failed\""));
    }

    #[test]
    fn route_dashboard_design_bundle_reports_uninitialized_workspace() {
        let workspace = TestWorkspace::new("dashboard-design-uninitialized");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/design-bundle",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("design bundle should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"uninitialized\""));
        assert!(body.contains(".mutagen/project.json"));
    }

    #[test]
    fn route_dashboard_design_bundle_lists_capsule_documents() {
        let workspace = TestWorkspace::new("dashboard-design-bundle");

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
            "/api/design-bundle",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("design bundle should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"total\": 6"));
        assert!(body.contains("\"id\": \"brief\""));
        assert!(body.contains("\"id\": \"prd\""));
        assert!(body.contains("\"id\": \"adr\""));
        assert!(body.contains("\"id\": \"ddd\""));
        assert!(body.contains("\"id\": \"isc\""));
        assert!(body.contains("\"id\": \"dsd\""));
        assert!(body.contains("# Product Requirements Document"));
    }

    #[test]
    fn route_dashboard_design_doc_updates_document_and_readiness() {
        let workspace = TestWorkspace::new("dashboard-design-doc");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        let body = br##"{
            "document":"prd",
            "content":"# Product Requirements Document\n\n## Direction\n\nCrew scheduler should let dispatchers manage shifts, absences, overtime, approvals, and live day-of changes from a compact operational dashboard with reliable preview and build feedback.\n"
        }"##;
        let (status, content_type, response) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/design-doc",
            body,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("design doc should update");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(response.contains("\"status\": \"updated\""));
        assert!(response.contains("\"status\": \"ready\""));

        let prd = fs::read_to_string(workspace.root.join("docs/PRD.md")).expect("PRD should read");
        assert!(prd.contains("compact operational dashboard"));
    }

    #[test]
    fn route_dashboard_design_doc_seed_writes_starter_document() {
        let workspace = TestWorkspace::new("dashboard-design-seed");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: Some("render".to_string()),
            force: false,
        })
        .expect("project create should succeed");

        let (status, content_type, response) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/design-doc-seed",
            br#"{"document":"adr"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("design doc should seed");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(response.contains("\"status\": \"seeded\""));
        assert!(response.contains("\"status\": \"ready\""));

        let adr = fs::read_to_string(workspace.root.join("docs/ADR.md")).expect("ADR should read");
        assert!(adr.contains("# Architecture Design Record"));
        assert!(adr.contains("Crew Scheduler"));
        assert!(adr.contains("render"));
    }

    #[test]
    fn route_dashboard_design_bundle_seed_seeds_all_draft_documents() {
        let workspace = TestWorkspace::new("dashboard-design-bundle-seed");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: Some("render".to_string()),
            force: false,
        })
        .expect("project create should succeed");

        fs::write(
            workspace.root.join(".mutagen/design/brief.md"),
            "crew scheduling",
        )
        .expect("brief draft should be written");
        fs::write(workspace.root.join("docs/PRD.md"), "short")
            .expect("PRD draft should be written");
        fs::write(workspace.root.join("docs/ADR.md"), "short")
            .expect("ADR draft should be written");
        fs::write(workspace.root.join("docs/DDD.md"), "short")
            .expect("DDD draft should be written");
        fs::write(workspace.root.join("docs/ISC.md"), "short")
            .expect("ISC draft should be written");
        fs::write(workspace.root.join("docs/DSD.md"), "short")
            .expect("DSD draft should be written");

        let (status, content_type, response) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/design-bundle-seed",
            b"{}",
            &workspace.root,
            HostKind::Stub,
        )
        .expect("design bundle should seed");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(response.contains("\"status\": \"seeded\""));
        assert!(response.contains("\"id\": \"brief\""));
        assert!(response.contains("\"id\": \"prd\""));
        assert!(response.contains("\"id\": \"adr\""));
        assert!(response.contains("\"id\": \"ddd\""));
        assert!(response.contains("\"id\": \"isc\""));
        assert!(response.contains("\"id\": \"dsd\""));
        assert!(response.contains("\"ready\": 6"));

        let brief = fs::read_to_string(workspace.root.join(".mutagen/design/brief.md"))
            .expect("brief should read");
        let prd = fs::read_to_string(workspace.root.join("docs/PRD.md")).expect("PRD should read");
        let dsd = fs::read_to_string(workspace.root.join("docs/DSD.md")).expect("DSD should read");

        assert!(brief.contains("Current direction"));
        assert!(prd.contains("Product Direction"));
        assert!(dsd.contains("Interaction Quality"));
    }

    #[test]
    fn route_dashboard_build_readiness_reports_uninitialized_workspace() {
        let workspace = TestWorkspace::new("dashboard-readiness-uninitialized");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/build-readiness",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("build readiness should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"uninitialized\""));
        assert!(body.contains("\"can_execute\": false"));
        assert!(body.contains("\"id\": \"capsule\""));
    }

    #[test]
    fn route_dashboard_build_readiness_reports_preflight_blockers() {
        let workspace = TestWorkspace::new("dashboard-readiness-blocked");

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
            "/api/build-readiness",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("build readiness should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"blocked\""));
        assert!(body.contains("\"can_execute\": false"));
        assert!(body.contains("\"id\": \"design_bundle\""));
        assert!(body.contains("\"id\": \"command_setup\""));
        assert!(body.contains("\"id\": \"command_test\""));
        assert!(body.contains("\"id\": \"command_build\""));
        assert!(body.contains("\"id\": \"queue_ready\""));
        assert!(body.contains("\"repair_action\": \"seed_design_bundle\""));
        assert!(body.contains("\"repair_action\": \"run_command\""));
        assert!(body.contains("\"repair_action\": \"focus_builder_queue_work\""));
        assert!(body.contains("\"kind\": \"setup\""));
    }

    #[test]
    fn route_dashboard_build_readiness_reports_missing_preview_command_as_blocker() {
        let workspace = TestWorkspace::new("dashboard-readiness-missing-preview-command");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        let capsule_path = workspace.root.join(".mutagen/project.json");
        let mut capsule: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&capsule_path).expect("capsule should read"))
                .expect("capsule should parse");
        capsule["commands"]["dev"] = serde_json::Value::String(String::new());
        fs::write(
            &capsule_path,
            serde_json::to_string_pretty(&capsule).expect("capsule should serialize"),
        )
        .expect("capsule should write");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/build-readiness",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("build readiness should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"blocked\""));
        assert!(body.contains("\"id\": \"preview_config\""));
        assert!(body.contains("project command `dev` is not configured"));
    }

    #[test]
    fn route_dashboard_inference_host_persists_selection() {
        let workspace = TestWorkspace::new("dashboard-inference-host");

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
            "/api/inference-host",
            br#"{"host":"codex"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("host update should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"updated\""));
        assert!(body.contains("\"selected_host\": \"codex\""));

        let settings = fs::read_to_string(
            workspace
                .root
                .join(".mutagen/state/dashboard-settings.json"),
        )
        .expect("settings should be written");
        assert!(settings.contains("\"host\": \"codex\""));

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/inference-host",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("host state should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"selected_host\": \"codex\""));
        assert!(body.contains("\"persisted\": true"));
    }

    #[test]
    fn route_dashboard_execute_feature_uses_persisted_inference_host() {
        let workspace = TestWorkspace::new("dashboard-inference-execute");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        let flow = feature_flow(ProjectFeatureFlowOptions {
            workspace_root: workspace.root.clone(),
            title: "Add due dates".to_string(),
            description: "Tasks should include optional due dates.".to_string(),
            force: false,
        })
        .expect("feature flow should succeed");

        route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/inference-host",
            br#"{"host":"codex"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("host update should render");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/execute-feature",
            format!(r#"{{"feature_id":"{}"}}"#, flow.feature_id).as_bytes(),
            &workspace.root,
            HostKind::Stub,
        )
        .expect("execute feature should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"feature_slice_ready\""));

        let active_state =
            fs::read_to_string(workspace.root.join(".mutagen/state/active-slice.json"))
                .expect("active state should be written");
        assert!(active_state.contains("\"host\": \"codex\""));

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
        assert!(body.contains("\"active_feature\""));
        assert!(body.contains("\"host\": \"codex\""));
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
            "{\"slice_id\":\"slice-001\",\"status\":\"completed\",\"completed_at\":\"2026-04-25T16:01:00Z\",\"host\":\"claude\"}\n",
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
        assert!(body.contains("slice-001 completed via claude"));
        assert!(body.contains("\"host\": \"claude\""));
        assert!(body.contains("build failed"));
    }

    #[test]
    fn route_dashboard_activity_feed_reports_active_host() {
        let workspace = TestWorkspace::new("dashboard-activity-active-host");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        let flow = feature_flow(ProjectFeatureFlowOptions {
            workspace_root: workspace.root.clone(),
            title: "Add due dates".to_string(),
            description: "Tasks should include optional due dates.".to_string(),
            force: false,
        })
        .expect("feature flow should succeed");

        route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/execute-feature",
            format!(r#"{{"feature_id":"{}","host":"codex"}}"#, flow.feature_id).as_bytes(),
            &workspace.root,
            HostKind::Stub,
        )
        .expect("execute feature should render");

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
        assert!(body.contains("\"kind\": \"active\""));
        assert!(body.contains("\"host\": \"codex\""));
        assert!(body.contains("via codex"));
    }

    #[test]
    fn route_dashboard_execution_run_dry_run_records_job_and_detail() {
        let workspace = TestWorkspace::new("dashboard-execution-dry-run");

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
            "/api/execution-run",
            br#"{"dry_run":true,"host":"stub"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("execution dry run should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"started\""));
        assert!(body.contains("\"status\": \"queue_clear\""));
        assert!(body.contains("dashboard-jobs"));

        let parsed: serde_json::Value =
            serde_json::from_str(&body).expect("execution response should parse");
        let job_id = parsed["job"]["id"]
            .as_str()
            .expect("job id should exist")
            .to_string();

        let (status, content_type, list_body) = route_dashboard_request(
            &tiny_http::Method::Get,
            "/api/execution-jobs?limit=5",
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("execution jobs should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(list_body.contains(&job_id));
        assert!(list_body.contains("\"status\": \"queue_clear\""));

        let detail_path = format!("/api/execution-job?id={job_id}");
        let (status, content_type, detail_body) = route_dashboard_request(
            &tiny_http::Method::Get,
            &detail_path,
            &[],
            &workspace.root,
            HostKind::Stub,
        )
        .expect("execution job detail should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(detail_body.contains("\"stdout_lines\""));
        assert!(detail_body.contains("\"dry_run\": true"));
        assert!(detail_body.contains("\"queue_clear\""));
    }

    #[test]
    fn route_dashboard_execution_cancel_marks_running_job_cancelled() {
        let workspace = TestWorkspace::new("dashboard-execution-cancel");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        let jobs_root = workspace.root.join(".mutagen/state/dashboard-jobs");
        fs::create_dir_all(&jobs_root).expect("jobs root should be created");
        let job_id = "execution-cancel-test";
        let stdout_path = jobs_root.join(format!("{job_id}.stdout.log"));
        let stderr_path = jobs_root.join(format!("{job_id}.stderr.log"));
        let metadata_path = jobs_root.join(format!("{job_id}.json"));

        fs::write(&stdout_path, "").expect("stdout should be written");
        fs::write(&stderr_path, "").expect("stderr should be written");
        fs::write(
            &metadata_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "id": job_id,
                "ok": true,
                "status": "running",
                "workspace_root": workspace.root.to_string_lossy(),
                "host": "stub",
                "dry_run": false,
                "pid": 999999,
                "command": ["bash", "noop"],
                "started_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "stdout_path": stdout_path.to_string_lossy(),
                "stderr_path": stderr_path.to_string_lossy(),
                "metadata_path": metadata_path.to_string_lossy(),
                "completed_count": 0
            }))
            .expect("job should serialize"),
        )
        .expect("job metadata should be written");

        let (status, content_type, body) = route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/execution-cancel",
            br#"{"id":"execution-cancel-test"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("execution cancel should render");

        assert_eq!(status, 200);
        assert_eq!(content_type, "application/json");
        assert!(body.contains("\"status\": \"cancelled\""));

        let updated = fs::read_to_string(&metadata_path).expect("metadata should read");
        assert!(updated.contains("\"status\": \"cancelled\""));
    }

    #[test]
    fn route_dashboard_activity_feed_reports_execution_jobs() {
        let workspace = TestWorkspace::new("dashboard-activity-execution");

        create_project(ProjectCreateOptions {
            workspace_root: workspace.root.clone(),
            name: "Crew Scheduler".to_string(),
            stack: "vite-express-sqlite".to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project create should succeed");

        route_dashboard_request(
            &tiny_http::Method::Post,
            "/api/execution-run",
            br#"{"dry_run":true,"host":"stub"}"#,
            &workspace.root,
            HostKind::Stub,
        )
        .expect("execution dry run should render");

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
        assert!(body.contains("\"kind\": \"execution\""));
        assert!(body.contains("execution queue_clear via stub"));
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
