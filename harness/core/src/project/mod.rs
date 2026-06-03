pub mod lock;
pub mod registry;
pub mod root;

use crate::adapter::HostKind;
use crate::queue::{HumanCheckNeeded, Slice, SliceStatus, TraceSet, VerificationSteps};
use crate::queue_readiness::{QUEUE_CONTRACT_HASH_BASIS, queue_contract_hash};
use crate::selected_slice::{
    PrepareSelectedSliceOptions, PrepareSelectedSliceResult, prepare_selected_slice,
};
use crate::state::ActiveSliceState;
use crate::validation::{load_queue_file, validate_queue};
use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessStatus, System};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[derive(Debug, Clone)]
pub struct ProjectInitOptions {
    pub workspace_root: PathBuf,
    pub name: String,
    pub stack: String,
    pub design_system: String,
    pub deploy_target: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectCreateOptions {
    pub workspace_root: PathBuf,
    pub name: String,
    pub stack: String,
    pub design_system: String,
    pub deploy_target: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectInspectOptions {
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProjectStatusOptions {
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProjectIntakeOptions {
    pub workspace_root: PathBuf,
    pub prompt: String,
    pub queue_feature: bool,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectAddFeatureOptions {
    pub workspace_root: PathBuf,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ProjectFeaturesOptions {
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProjectPlanFeatureOptions {
    pub workspace_root: PathBuf,
    pub feature_id: String,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectFeatureStatusOptions {
    pub workspace_root: PathBuf,
    pub feature_id: String,
}

#[derive(Debug, Clone)]
pub struct ProjectSliceFeatureOptions {
    pub workspace_root: PathBuf,
    pub feature_id: String,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectEnqueueFeatureOptions {
    pub workspace_root: PathBuf,
    pub feature_id: String,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectFeatureFlowOptions {
    pub workspace_root: PathBuf,
    pub title: String,
    pub description: String,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectExecuteFeatureOptions {
    pub workspace_root: PathBuf,
    pub feature_id: String,
    pub host: HostKind,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectFeatureProgressOptions {
    pub workspace_root: PathBuf,
    pub feature_id: String,
}

#[derive(Debug, Clone)]
pub struct ProjectDashboardOptions {
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProjectApplyBlueprintOptions {
    pub workspace_root: PathBuf,
    pub stack: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectScaffoldOptions {
    pub workspace_root: PathBuf,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectRepairOptions {
    pub workspace_root: PathBuf,
    pub scaffold: bool,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectRunCommandOptions {
    pub workspace_root: PathBuf,
    pub kind: ProjectCommandKind,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct ProjectVerifyGeneratedOptions {
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProjectDoctorOptions {
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProjectPreviewPlanOptions {
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProjectPreviewLifecycleOptions {
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProjectPreviewCheckOptions {
    pub workspace_root: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ProjectCommandKind {
    Setup,
    Dev,
    Test,
    Build,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCapsule {
    pub schema_version: u32,
    pub name: String,
    pub stack: String,
    pub design_system: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy_target: Option<String>,
    pub documents: ProjectDocuments,
    pub queue: String,
    pub workflow_config: String,
    pub design: ProjectDesignBundle,
    pub state: ProjectStateFiles,
    pub commands: ProjectCommands,
    #[serde(default)]
    pub preview: ProjectPreview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDocuments {
    pub prd: String,
    pub adr: String,
    pub ddd: String,
    pub isc: String,
    pub dsd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDesignBundle {
    pub root: String,
    pub brief: String,
    pub tokens: String,
    pub components: String,
    pub screens: String,
    pub screenshots: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectStateFiles {
    pub project_state: String,
    pub decisions_log: String,
    pub build_log: String,
    pub deployments_log: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectCommands {
    pub setup: String,
    pub dev: String,
    pub test: String,
    pub build: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPreview {
    pub url: String,
    pub command_kind: ProjectCommandKind,
    pub readiness_timeout_seconds: u32,
}

impl Default for ProjectPreview {
    fn default() -> Self {
        Self {
            url: String::new(),
            command_kind: ProjectCommandKind::Dev,
            readiness_timeout_seconds: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectInitResult {
    pub ok: bool,
    pub status: String,
    pub capsule_path: String,
    pub capsule: ProjectCapsule,
    pub created_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectCreateResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub init: ProjectInitResult,
    pub blueprint: ProjectApplyBlueprintResult,
    pub scaffold: ProjectScaffoldResult,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectInspectResult {
    pub ok: bool,
    pub status: String,
    pub capsule_path: String,
    pub capsule: ProjectCapsule,
    pub missing_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectStatusResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub stack: String,
    pub capsule_ok: bool,
    pub scaffold_ok: bool,
    pub doctor_ok: bool,
    pub preview: ProjectPreviewLifecycleResult,
    pub missing_paths: Vec<String>,
    pub missing_scaffold_paths: Vec<String>,
    pub doctor: ProjectDoctorResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_build_log_entry: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDesignBriefSummary {
    pub path: String,
    pub exists: bool,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectIntakeResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub intake_mode: String,
    pub title: String,
    pub prompt: String,
    pub brief: ProjectDesignBriefSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_flow: Option<ProjectFeatureFlowResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFeatureIntent {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub target_stack: String,
    pub created_at: String,
    pub brief_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectAddFeatureResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub feature: ProjectFeatureIntent,
    pub log_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectFeaturesResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub log_path: String,
    pub features: Vec<ProjectFeatureIntent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFeaturePlan {
    pub feature_id: String,
    pub title: String,
    pub status: String,
    pub target_stack: String,
    pub plan_path: String,
    pub generated_at: String,
    pub summary: String,
    pub target_paths: Vec<String>,
    pub verification_commands: Vec<String>,
    pub steps: Vec<ProjectFeaturePlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFeaturePlanStep {
    pub id: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectPlanFeatureResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub feature: ProjectFeatureIntent,
    pub plan: ProjectFeaturePlan,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectFeatureStatusResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub feature: ProjectFeatureIntent,
    pub brief_path: String,
    pub brief_exists: bool,
    pub plan_path: String,
    pub plan_exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<ProjectFeaturePlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFeatureSliceManifest {
    pub schema_version: u32,
    pub feature_id: String,
    pub title: String,
    pub status: String,
    pub target_stack: String,
    pub source_plan_path: String,
    pub slices_path: String,
    pub generated_at: String,
    pub slices: Vec<ProjectFeatureSlice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectFeatureSlice {
    pub id: String,
    pub title: String,
    pub status: String,
    pub plan_step_id: String,
    pub summary: String,
    pub target_paths: Vec<String>,
    pub verification_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSliceFeatureResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub feature: ProjectFeatureIntent,
    pub slices_path: String,
    pub manifest: ProjectFeatureSliceManifest,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectEnqueueFeatureResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub feature: ProjectFeatureIntent,
    pub queue_path: String,
    pub slices_path: String,
    pub prd_path: String,
    pub enqueued_slice_ids: Vec<String>,
    pub replaced_slice_ids: Vec<String>,
    pub queue_slice_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectFeatureFlowResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub feature_id: String,
    pub add_feature: ProjectAddFeatureResult,
    pub plan_feature: ProjectPlanFeatureResult,
    pub slice_feature: ProjectSliceFeatureResult,
    pub enqueue_feature: ProjectEnqueueFeatureResult,
}

#[derive(Debug, Serialize)]
pub struct ProjectExecuteFeatureResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub feature_id: String,
    pub queue_path: String,
    pub workflow_config_path: String,
    pub active_state_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_slice_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepare: Option<PrepareSelectedSliceResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectFeatureProgressResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub feature: ProjectFeatureIntent,
    pub queue_path: String,
    pub active_state_path: String,
    pub total: usize,
    pub counts: ProjectFeatureProgressCounts,
    pub slices: Vec<ProjectFeatureProgressSlice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_slice: Option<ProjectFeatureActiveSlice>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProjectFeatureProgressCounts {
    pub pending: usize,
    pub in_progress: usize,
    pub blocked_retry: usize,
    pub completed: usize,
    pub escalated: usize,
    pub refused: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectFeatureProgressSlice {
    pub id: String,
    pub title: String,
    pub status: SliceStatus,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectFeatureActiveSlice {
    pub id: String,
    pub title: String,
    pub stage: String,
    pub active_agent: String,
    pub host: HostKind,
    pub evidence_bundle_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDashboardResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub project: ProjectStatusResult,
    pub project_brief: ProjectDesignBriefSummary,
    pub feature_backlog: ProjectDashboardFeatureBacklog,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_feature: Option<ProjectFeatureProgressResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDashboardFeatureBacklog {
    pub total: usize,
    pub queued: usize,
    pub planned: usize,
    pub ready: usize,
    pub in_queue: usize,
    pub features: Vec<ProjectDashboardFeatureItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDashboardFeatureItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlueprintCatalogResult {
    pub ok: bool,
    pub blueprints: Vec<StackBlueprint>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectApplyBlueprintResult {
    pub ok: bool,
    pub status: String,
    pub capsule_path: String,
    pub blueprint: StackBlueprint,
    pub capsule: ProjectCapsule,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectScaffoldResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub stack: String,
    pub created_paths: Vec<String>,
    pub overwritten_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectRepairResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub repaired_paths: Vec<String>,
    pub overwritten_paths: Vec<String>,
    pub skipped_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectRunCommandResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub command_kind: ProjectCommandKind,
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub build_log_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectVerifyGeneratedResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub steps: Vec<ProjectVerifyGeneratedStep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectVerifyGeneratedStep {
    pub name: String,
    pub ok: bool,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDoctorResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub stack: String,
    pub checks: Vec<ProjectDoctorCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDoctorCheck {
    pub executable: String,
    pub ok: bool,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectPreviewPlanResult {
    pub ok: bool,
    pub status: String,
    pub workspace_root: String,
    pub stack: String,
    pub url: String,
    pub command_kind: ProjectCommandKind,
    pub command: String,
    pub readiness_timeout_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectPreviewState {
    pub pid: u32,
    pub url: String,
    pub command_kind: ProjectCommandKind,
    pub command: String,
    pub started_at: String,
    pub state_path: String,
    pub log_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectPreviewLifecycleResult {
    pub ok: bool,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub running: bool,
    pub ready: bool,
    pub url: String,
    pub command: String,
    pub state_path: String,
    pub log_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectPreviewCheckResult {
    pub ok: bool,
    pub status: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub running: bool,
    pub ready: bool,
    pub url: String,
    pub command: String,
    pub state_path: String,
    pub log_path: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
struct ProjectCommandLogEntry {
    event: String,
    command_kind: ProjectCommandKind,
    command: String,
    status: String,
    exit_code: Option<i32>,
    recorded_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StackBlueprint {
    pub stack: String,
    pub label: String,
    pub description: String,
    pub commands: ProjectCommands,
    pub preview: ProjectPreview,
}

impl ProjectCapsule {
    fn new(
        name: String,
        stack: String,
        design_system: String,
        deploy_target: Option<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            name,
            stack,
            design_system,
            deploy_target,
            documents: ProjectDocuments {
                prd: "docs/PRD.md".to_string(),
                adr: "docs/ADR.md".to_string(),
                ddd: "docs/DDD.md".to_string(),
                isc: "docs/ISC.md".to_string(),
                dsd: "docs/DSD.md".to_string(),
            },
            queue: "slices/queue.json".to_string(),
            workflow_config: ".claude/workflow.json".to_string(),
            design: ProjectDesignBundle {
                root: ".mutagen/design".to_string(),
                brief: ".mutagen/design/brief.md".to_string(),
                tokens: ".mutagen/design/tokens.json".to_string(),
                components: ".mutagen/design/components.json".to_string(),
                screens: ".mutagen/design/screens".to_string(),
                screenshots: ".mutagen/design/screenshots".to_string(),
            },
            state: ProjectStateFiles {
                project_state: "project_state.md".to_string(),
                decisions_log: ".mutagen/state/decisions.jsonl".to_string(),
                build_log: ".mutagen/state/build-log.jsonl".to_string(),
                deployments_log: ".mutagen/state/deployments.jsonl".to_string(),
            },
            commands: ProjectCommands {
                setup: String::new(),
                dev: String::new(),
                test: String::new(),
                build: String::new(),
            },
            preview: ProjectPreview::default(),
        }
    }

    fn expected_paths(&self) -> Vec<&str> {
        vec![
            &self.documents.prd,
            &self.documents.adr,
            &self.documents.ddd,
            &self.documents.isc,
            &self.documents.dsd,
            &self.queue,
            &self.workflow_config,
            &self.design.brief,
            &self.design.tokens,
            &self.design.components,
            &self.design.screens,
            &self.design.screenshots,
            &self.state.project_state,
            &self.state.decisions_log,
            &self.state.build_log,
            &self.state.deployments_log,
        ]
    }
}

pub fn init_project(options: ProjectInitOptions) -> Result<ProjectInitResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");

    if capsule_path.exists() && !options.force {
        bail!(
            "project capsule already exists at {}; pass --force to overwrite it",
            display_path(&capsule_path)
        );
    }

    fs::create_dir_all(&workspace_root).with_context(|| {
        format!(
            "failed to create workspace root at {}",
            display_path(&workspace_root)
        )
    })?;

    let capsule = ProjectCapsule::new(
        options.name,
        options.stack,
        options.design_system,
        options.deploy_target,
    );

    let mut created_paths = Vec::new();
    write_if_missing(
        &workspace_root,
        &capsule.documents.prd,
        "# Product Requirements Document\n\n",
        options.force,
        &mut created_paths,
    )?;
    write_if_missing(
        &workspace_root,
        &capsule.documents.adr,
        "# Architecture Design Record\n\n",
        options.force,
        &mut created_paths,
    )?;
    write_if_missing(
        &workspace_root,
        &capsule.documents.ddd,
        "# Domain Model\n\n",
        options.force,
        &mut created_paths,
    )?;
    write_if_missing(
        &workspace_root,
        &capsule.documents.isc,
        "# Implied Systems Contract\n\n",
        options.force,
        &mut created_paths,
    )?;
    write_if_missing(
        &workspace_root,
        &capsule.documents.dsd,
        "# Design Style Guide\n\n",
        options.force,
        &mut created_paths,
    )?;
    write_json_if_missing(
        &workspace_root,
        &capsule.queue,
        &json!({
            "version": 1,
            "generated_by": "mutagen-harness",
            "pipeline_mode": "full",
            "planning_advisories": [],
            "slices": []
        }),
        options.force,
        &mut created_paths,
    )?;
    write_json_if_missing(
        &workspace_root,
        &capsule.workflow_config,
        &json!({
            "pipeline_mode": "full",
            "max_parallel_slices": 1,
            "review": {
                "max_retries": 2,
                "max_micro_corrections": 1
            }
        }),
        options.force,
        &mut created_paths,
    )?;
    write_if_missing(
        &workspace_root,
        &capsule.design.brief,
        "# Design Brief\n\n",
        options.force,
        &mut created_paths,
    )?;
    write_json_if_missing(
        &workspace_root,
        &capsule.design.tokens,
        &json!({ "version": 1, "tokens": {} }),
        options.force,
        &mut created_paths,
    )?;
    write_json_if_missing(
        &workspace_root,
        &capsule.design.components,
        &json!({ "version": 1, "components": [] }),
        options.force,
        &mut created_paths,
    )?;
    create_dir_if_missing(&workspace_root, &capsule.design.screens, &mut created_paths)?;
    create_dir_if_missing(
        &workspace_root,
        &capsule.design.screenshots,
        &mut created_paths,
    )?;
    write_if_missing(
        &workspace_root,
        &capsule.state.project_state,
        "# Project State\n\n",
        options.force,
        &mut created_paths,
    )?;
    write_if_missing(
        &workspace_root,
        &capsule.state.decisions_log,
        "",
        options.force,
        &mut created_paths,
    )?;
    write_if_missing(
        &workspace_root,
        &capsule.state.build_log,
        "",
        options.force,
        &mut created_paths,
    )?;
    write_if_missing(
        &workspace_root,
        &capsule.state.deployments_log,
        "",
        options.force,
        &mut created_paths,
    )?;

    write_json(&capsule_path, &capsule)?;
    created_paths.push(relative_display(".mutagen/project.json"));

    Ok(ProjectInitResult {
        ok: true,
        status: "initialized".to_string(),
        capsule_path: display_path(&capsule_path),
        capsule,
        created_paths,
    })
}

pub fn create_project(options: ProjectCreateOptions) -> Result<ProjectCreateResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let blueprint = blueprint_for(&options.stack)?;
    let mut planned_capsule = ProjectCapsule::new(
        options.name.clone(),
        options.stack.clone(),
        options.design_system.clone(),
        options.deploy_target.clone(),
    );
    planned_capsule.stack = blueprint.stack;
    planned_capsule.commands = blueprint.commands;
    planned_capsule.preview = blueprint.preview;
    let planned_files = scaffold_files(&planned_capsule)?;

    let existing_paths = scaffold_existing_paths(&workspace_root, &planned_files);
    if !existing_paths.is_empty() && !options.force {
        bail!(
            "create would overwrite existing scaffold paths: {}; pass --force to replace them",
            existing_paths.join(", ")
        );
    }

    let init = init_project(ProjectInitOptions {
        workspace_root: workspace_root.clone(),
        name: options.name,
        stack: options.stack,
        design_system: options.design_system,
        deploy_target: options.deploy_target,
        force: options.force,
    })?;
    let blueprint = apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace_root.clone(),
        stack: None,
    })?;
    let scaffold = scaffold_project(ProjectScaffoldOptions {
        workspace_root: workspace_root.clone(),
        force: options.force,
    })?;

    Ok(ProjectCreateResult {
        ok: init.ok && blueprint.ok && scaffold.ok,
        status: "created".to_string(),
        workspace_root: display_path(&workspace_root),
        init,
        blueprint,
        scaffold,
    })
}

pub fn inspect_project(options: ProjectInspectOptions) -> Result<ProjectInspectResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;

    let missing_paths = capsule
        .expected_paths()
        .into_iter()
        .filter(|path| !workspace_root.join(path).exists())
        .map(relative_display)
        .collect::<Vec<_>>();

    let status = if missing_paths.is_empty() {
        "ready"
    } else {
        "incomplete"
    };

    Ok(ProjectInspectResult {
        ok: missing_paths.is_empty(),
        status: status.to_string(),
        capsule_path: display_path(&capsule_path),
        capsule,
        missing_paths,
    })
}

pub fn status_project(options: ProjectStatusOptions) -> Result<ProjectStatusResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let inspect = inspect_project(ProjectInspectOptions {
        workspace_root: workspace_root.clone(),
    })?;
    let scaffold_files = scaffold_files(&inspect.capsule)?;
    let missing_scaffold_paths = scaffold_files
        .iter()
        .filter(|file| !workspace_root.join(&file.relative_path).exists())
        .map(|file| relative_display(&file.relative_path))
        .collect::<Vec<_>>();
    let doctor = doctor_project(ProjectDoctorOptions {
        workspace_root: workspace_root.clone(),
    })?;
    let preview = preview_status(ProjectPreviewLifecycleOptions {
        workspace_root: workspace_root.clone(),
    })?;
    let last_build_log_entry =
        last_build_log_entry(&workspace_root.join(&inspect.capsule.state.build_log))?;
    let scaffold_ok = missing_scaffold_paths.is_empty();
    let ok = inspect.ok && scaffold_ok && doctor.ok;

    Ok(ProjectStatusResult {
        ok,
        status: if ok { "ready" } else { "attention" }.to_string(),
        workspace_root: display_path(&workspace_root),
        stack: inspect.capsule.stack,
        capsule_ok: inspect.ok,
        scaffold_ok,
        doctor_ok: doctor.ok,
        preview,
        missing_paths: inspect.missing_paths,
        missing_scaffold_paths,
        doctor,
        last_build_log_entry,
    })
}

pub fn project_intake(options: ProjectIntakeOptions) -> Result<ProjectIntakeResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let prompt = options.prompt.trim();

    if prompt.is_empty() {
        bail!("project prompt is required");
    }

    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;
    let brief_path = workspace_root.join(&capsule.design.brief);
    let title = derive_project_intake_title(prompt);

    write_project_brief_intake(&brief_path, &title, prompt)?;
    let brief = load_project_brief_summary(&brief_path)?;

    let mut ok = true;
    let mut status = "brief_updated".to_string();
    let mut feature_flow_result = None;
    let mut queue_error = None;

    if options.queue_feature {
        match feature_flow(ProjectFeatureFlowOptions {
            workspace_root: workspace_root.clone(),
            title: title.clone(),
            description: prompt.to_string(),
            force: options.force,
        }) {
            Ok(result) => {
                status = "brief_updated_and_feature_flow_ready".to_string();
                feature_flow_result = Some(result);
            }
            Err(error) => {
                ok = false;
                status = "brief_updated_queue_failed".to_string();
                queue_error = Some(error.to_string());
            }
        }
    }

    Ok(ProjectIntakeResult {
        ok,
        status,
        workspace_root: display_path(&workspace_root),
        intake_mode: if options.queue_feature {
            "brief_and_queue".to_string()
        } else {
            "brief_only".to_string()
        },
        title,
        prompt: prompt.to_string(),
        brief,
        feature_flow: feature_flow_result,
        queue_error,
    })
}

pub fn add_feature(options: ProjectAddFeatureOptions) -> Result<ProjectAddFeatureResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let title = options.title.trim();

    if title.is_empty() {
        bail!("feature title is required");
    }

    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;
    let created_at = now_rfc3339()?;
    let id = feature_id(title);
    let brief_path = format!(".mutagen/features/{id}/brief.md");
    let log_path = workspace_root.join(".mutagen/state/features.jsonl");
    let feature = ProjectFeatureIntent {
        id: id.clone(),
        title: title.to_string(),
        description: options.description.trim().to_string(),
        status: "queued".to_string(),
        target_stack: capsule.stack,
        created_at,
        brief_path: brief_path.clone(),
    };

    write_feature_brief(&workspace_root, &feature)?;
    append_feature_log(&log_path, &feature)?;

    Ok(ProjectAddFeatureResult {
        ok: true,
        status: "feature_queued".to_string(),
        workspace_root: display_path(&workspace_root),
        feature,
        log_path: display_path(&log_path),
    })
}

pub fn list_features(options: ProjectFeaturesOptions) -> Result<ProjectFeaturesResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let log_path = workspace_root.join(".mutagen/state/features.jsonl");
    let features = read_feature_log(&log_path)?;

    Ok(ProjectFeaturesResult {
        ok: true,
        status: if features.is_empty() {
            "empty".to_string()
        } else {
            "ready".to_string()
        },
        workspace_root: display_path(&workspace_root),
        log_path: display_path(&log_path),
        features,
    })
}

pub fn plan_feature(options: ProjectPlanFeatureOptions) -> Result<ProjectPlanFeatureResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let log_path = workspace_root.join(".mutagen/state/features.jsonl");
    let features = read_feature_log(&log_path)?;
    let feature = features
        .into_iter()
        .find(|feature| feature.id == options.feature_id)
        .ok_or_else(|| anyhow::anyhow!("feature `{}` was not found", options.feature_id))?;
    let plan_path = format!(".mutagen/features/{}/plan.json", feature.id);
    let absolute_plan_path = workspace_root.join(&plan_path);

    if absolute_plan_path.exists() && !options.force {
        bail!(
            "feature plan already exists at {}; pass --force to overwrite it",
            display_path(&absolute_plan_path)
        );
    }

    let plan = feature_plan(&feature, plan_path)?;
    write_json(&absolute_plan_path, &plan)?;

    Ok(ProjectPlanFeatureResult {
        ok: true,
        status: "feature_planned".to_string(),
        workspace_root: display_path(&workspace_root),
        feature,
        plan,
    })
}

pub fn feature_status(options: ProjectFeatureStatusOptions) -> Result<ProjectFeatureStatusResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let log_path = workspace_root.join(".mutagen/state/features.jsonl");
    let features = read_feature_log(&log_path)?;
    let feature = features
        .into_iter()
        .find(|feature| feature.id == options.feature_id)
        .ok_or_else(|| anyhow::anyhow!("feature `{}` was not found", options.feature_id))?;
    let brief_path = workspace_root.join(&feature.brief_path);
    let plan_path = workspace_root.join(format!(".mutagen/features/{}/plan.json", feature.id));
    let brief_exists = brief_path.exists();
    let plan_exists = plan_path.exists();
    let plan = if plan_exists {
        Some(load_feature_plan(&plan_path)?)
    } else {
        None
    };
    let ready = brief_exists && plan_exists;

    Ok(ProjectFeatureStatusResult {
        ok: ready,
        status: if ready { "ready" } else { "needs_plan" }.to_string(),
        workspace_root: display_path(&workspace_root),
        feature,
        brief_path: display_path(&brief_path),
        brief_exists,
        plan_path: display_path(&plan_path),
        plan_exists,
        plan,
    })
}

pub fn slice_feature(options: ProjectSliceFeatureOptions) -> Result<ProjectSliceFeatureResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let log_path = workspace_root.join(".mutagen/state/features.jsonl");
    let features = read_feature_log(&log_path)?;
    let feature = features
        .into_iter()
        .find(|feature| feature.id == options.feature_id)
        .ok_or_else(|| anyhow::anyhow!("feature `{}` was not found", options.feature_id))?;
    let plan_path = workspace_root.join(format!(".mutagen/features/{}/plan.json", feature.id));

    if !plan_path.exists() {
        bail!(
            "feature plan is missing at {}; run plan-feature first",
            display_path(&plan_path)
        );
    }

    let slices_path = workspace_root.join(format!(".mutagen/features/{}/slices.json", feature.id));

    if slices_path.exists() && !options.force {
        bail!(
            "feature slices already exist at {}; pass --force to overwrite them",
            display_path(&slices_path)
        );
    }

    let plan = load_feature_plan(&plan_path)?;
    let manifest = feature_slice_manifest(&feature, &plan)?;
    write_json(&slices_path, &manifest)?;

    Ok(ProjectSliceFeatureResult {
        ok: true,
        status: "feature_sliced".to_string(),
        workspace_root: display_path(&workspace_root),
        feature,
        slices_path: display_path(&slices_path),
        manifest,
    })
}

pub fn enqueue_feature(
    options: ProjectEnqueueFeatureOptions,
) -> Result<ProjectEnqueueFeatureResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;
    let log_path = workspace_root.join(".mutagen/state/features.jsonl");
    let features = read_feature_log(&log_path)?;
    let feature = features
        .into_iter()
        .find(|feature| feature.id == options.feature_id)
        .ok_or_else(|| anyhow::anyhow!("feature `{}` was not found", options.feature_id))?;
    let slices_path = workspace_root.join(format!(".mutagen/features/{}/slices.json", feature.id));

    if !slices_path.exists() {
        bail!(
            "feature slices are missing at {}; run slice-feature first",
            display_path(&slices_path)
        );
    }

    let manifest = load_feature_slice_manifest(&slices_path)?;
    let queue_path = workspace_root.join(&capsule.queue);
    let mut queue = load_queue_file(&queue_path)?;
    let queue_slices = feature_queue_slices(&feature, &manifest);
    let enqueued_slice_ids = queue_slices
        .iter()
        .map(|slice| slice.id.clone())
        .collect::<Vec<_>>();
    let existing_ids = queue
        .slices
        .iter()
        .filter(|slice| enqueued_slice_ids.contains(&slice.id))
        .map(|slice| slice.id.clone())
        .collect::<Vec<_>>();

    if !existing_ids.is_empty() && !options.force {
        bail!(
            "queue already contains feature slices: {}; pass --force to replace them",
            existing_ids.join(", ")
        );
    }

    if !existing_ids.is_empty() {
        queue
            .slices
            .retain(|slice| !enqueued_slice_ids.contains(&slice.id));
    }

    queue.slices.extend(queue_slices);
    queue.generated_at = now_rfc3339()?;
    queue.generated_by = "mutagen-harness project enqueue-feature".to_string();

    let validation = validate_queue(&queue);
    if !validation.ok {
        let messages = validation
            .issues
            .iter()
            .filter(|issue| issue.level == crate::validation::ValidationLevel::Error)
            .map(|issue| issue.message.clone())
            .collect::<Vec<_>>();
        bail!(
            "feature queue import produced an invalid queue: {}",
            messages.join("; ")
        );
    }

    let prd_path = workspace_root.join(&capsule.documents.prd);
    ensure_feature_prd_section(&prd_path, &feature, &manifest)?;
    write_json(&queue_path, &queue)?;
    write_queue_validation_report(
        &workspace_root.join(".mutagen/state/queue-validation.json"),
        &queue_path,
        &validation,
    )?;

    Ok(ProjectEnqueueFeatureResult {
        ok: true,
        status: if existing_ids.is_empty() {
            "feature_enqueued".to_string()
        } else {
            "feature_reenqueued".to_string()
        },
        workspace_root: display_path(&workspace_root),
        feature,
        queue_path: display_path(&queue_path),
        slices_path: display_path(&slices_path),
        prd_path: display_path(&prd_path),
        enqueued_slice_ids,
        replaced_slice_ids: existing_ids,
        queue_slice_count: queue.slices.len(),
    })
}

fn write_queue_validation_report(
    path: &Path,
    queue_path: &Path,
    validation: &crate::validation::QueueValidationReport,
) -> Result<()> {
    let mut report = serde_json::to_value(validation).context("failed to serialize validation")?;
    report["queue"] = json!(display_path(queue_path));
    report["queue_contract_hash"] = json!(queue_contract_hash(queue_path)?);
    report["queue_contract_hash_basis"] = json!(QUEUE_CONTRACT_HASH_BASIS);
    report["queue_contract_hash_algorithm"] = json!("sha1");
    write_json(path, &report)
}

pub fn feature_flow(options: ProjectFeatureFlowOptions) -> Result<ProjectFeatureFlowResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let add_feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace_root.clone(),
        title: options.title,
        description: options.description,
    })?;
    let feature_id = add_feature.feature.id.clone();
    let plan_feature = plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace_root.clone(),
        feature_id: feature_id.clone(),
        force: options.force,
    })?;
    let slice_feature = slice_feature(ProjectSliceFeatureOptions {
        workspace_root: workspace_root.clone(),
        feature_id: feature_id.clone(),
        force: options.force,
    })?;
    let enqueue_feature = enqueue_feature(ProjectEnqueueFeatureOptions {
        workspace_root: workspace_root.clone(),
        feature_id: feature_id.clone(),
        force: options.force,
    })?;

    Ok(ProjectFeatureFlowResult {
        ok: true,
        status: "feature_flow_ready".to_string(),
        workspace_root: display_path(&workspace_root),
        feature_id,
        add_feature,
        plan_feature,
        slice_feature,
        enqueue_feature,
    })
}

pub fn execute_feature(
    options: ProjectExecuteFeatureOptions,
) -> Result<ProjectExecuteFeatureResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;
    let log_path = workspace_root.join(".mutagen/state/features.jsonl");
    let features = read_feature_log(&log_path)?;
    let feature = features
        .into_iter()
        .find(|feature| feature.id == options.feature_id)
        .ok_or_else(|| anyhow::anyhow!("feature `{}` was not found", options.feature_id))?;
    let queue_path = workspace_root.join(&capsule.queue);
    let queue_validation_path = workspace_root.join(".mutagen/state/queue-validation.json");
    let workflow_config_path = workspace_root.join(&capsule.workflow_config);
    let active_state_path = workspace_root.join(".mutagen/state/active-slice.json");
    let queue = load_queue_file(&queue_path)?;
    let selected_slice_id = next_feature_slice_id(&queue.slices, &feature.id);

    let Some(selected_slice_id) = selected_slice_id else {
        return Ok(ProjectExecuteFeatureResult {
            ok: true,
            status: "feature_complete".to_string(),
            workspace_root: display_path(&workspace_root),
            feature_id: feature.id,
            queue_path: display_path(&queue_path),
            workflow_config_path: display_path(&workflow_config_path),
            active_state_path: display_path(&active_state_path),
            selected_slice_id: None,
            prepare: None,
        });
    };

    let prepare = prepare_selected_slice(PrepareSelectedSliceOptions {
        workspace_root: workspace_root.clone(),
        queue_path: queue_path.clone(),
        queue_validation_path,
        workflow_config_path: workflow_config_path.clone(),
        active_state_path: active_state_path.clone(),
        slice_id: selected_slice_id.clone(),
        host: options.host,
        dry_run: options.dry_run,
    })?;
    let ok = matches!(prepare, PrepareSelectedSliceResult::Ready { .. });

    Ok(ProjectExecuteFeatureResult {
        ok,
        status: if ok {
            "feature_slice_ready".to_string()
        } else {
            "feature_slice_blocked".to_string()
        },
        workspace_root: display_path(&workspace_root),
        feature_id: feature.id,
        queue_path: display_path(&queue_path),
        workflow_config_path: display_path(&workflow_config_path),
        active_state_path: display_path(&active_state_path),
        selected_slice_id: Some(selected_slice_id),
        prepare: Some(prepare),
    })
}

pub fn feature_progress(
    options: ProjectFeatureProgressOptions,
) -> Result<ProjectFeatureProgressResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;
    let log_path = workspace_root.join(".mutagen/state/features.jsonl");
    let features = read_feature_log(&log_path)?;
    let feature = features
        .into_iter()
        .find(|feature| feature.id == options.feature_id)
        .ok_or_else(|| anyhow::anyhow!("feature `{}` was not found", options.feature_id))?;
    let queue_path = workspace_root.join(&capsule.queue);
    let active_state_path = workspace_root.join(".mutagen/state/active-slice.json");
    let queue = load_queue_file(&queue_path)?;
    let feature_slices = feature_slices(&queue.slices, &feature.id);
    let counts = feature_progress_counts(&feature_slices);
    let total = feature_slices.len();
    let active_slice = load_feature_active_slice(&active_state_path, &feature.id)?;
    let status = feature_progress_status(total, &counts);
    let ok = total > 0 && counts.escalated == 0 && counts.refused == 0;

    Ok(ProjectFeatureProgressResult {
        ok,
        status,
        workspace_root: display_path(&workspace_root),
        feature,
        queue_path: display_path(&queue_path),
        active_state_path: display_path(&active_state_path),
        total,
        counts,
        slices: feature_slices
            .into_iter()
            .map(|slice| ProjectFeatureProgressSlice {
                id: slice.id,
                title: slice.title,
                status: slice.status,
                depends_on: slice.depends_on,
            })
            .collect(),
        active_slice,
    })
}

pub fn dashboard_project(options: ProjectDashboardOptions) -> Result<ProjectDashboardResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;
    let project = status_project(ProjectStatusOptions {
        workspace_root: workspace_root.clone(),
    })?;
    let features = list_features(ProjectFeaturesOptions {
        workspace_root: workspace_root.clone(),
    })?;
    let active_feature = active_feature_progress(&workspace_root)?;
    let feature_backlog = dashboard_feature_backlog(&workspace_root, features.features)?;
    let project_brief = load_project_brief_summary(&workspace_root.join(&capsule.design.brief))?;

    Ok(ProjectDashboardResult {
        ok: true,
        status: project.status.clone(),
        workspace_root: display_path(&workspace_root),
        project,
        project_brief,
        feature_backlog,
        active_feature,
    })
}

pub fn list_blueprints() -> BlueprintCatalogResult {
    BlueprintCatalogResult {
        ok: true,
        blueprints: blueprint_catalog(),
    }
}

pub fn apply_blueprint(
    options: ProjectApplyBlueprintOptions,
) -> Result<ProjectApplyBlueprintResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");
    let mut capsule = load_capsule(&capsule_path)?;
    let stack = options.stack.unwrap_or_else(|| capsule.stack.clone());
    let blueprint = blueprint_for(&stack)?;

    capsule.stack = blueprint.stack.clone();
    capsule.commands = blueprint.commands.clone();
    capsule.preview = blueprint.preview.clone();
    write_json(&capsule_path, &capsule)?;

    Ok(ProjectApplyBlueprintResult {
        ok: true,
        status: "blueprint_applied".to_string(),
        capsule_path: display_path(&capsule_path),
        blueprint,
        capsule,
    })
}

pub fn scaffold_project(options: ProjectScaffoldOptions) -> Result<ProjectScaffoldResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;
    let files = scaffold_files(&capsule)?;

    let existing_paths = scaffold_existing_paths(&workspace_root, &files);

    if !existing_paths.is_empty() && !options.force {
        bail!(
            "scaffold would overwrite existing paths: {}; pass --force to replace them",
            existing_paths.join(", ")
        );
    }

    let mut created_paths = Vec::new();
    let mut overwritten_paths = Vec::new();

    for file in files {
        let path = workspace_root.join(&file.relative_path);
        let existed = path.exists();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create parent directory for {}",
                    display_path(&path)
                )
            })?;
        }

        fs::write(&path, file.body)
            .with_context(|| format!("failed to write {}", display_path(&path)))?;

        if existed {
            overwritten_paths.push(relative_display(&file.relative_path));
        } else {
            created_paths.push(relative_display(&file.relative_path));
        }
    }

    Ok(ProjectScaffoldResult {
        ok: true,
        status: if overwritten_paths.is_empty() {
            "scaffolded".to_string()
        } else {
            "scaffolded_with_overwrites".to_string()
        },
        workspace_root: display_path(&workspace_root),
        stack: capsule.stack,
        created_paths,
        overwritten_paths,
    })
}

pub fn repair_project(options: ProjectRepairOptions) -> Result<ProjectRepairResult> {
    if !options.scaffold {
        bail!("no repair target selected; pass --scaffold");
    }

    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;
    let files = scaffold_files(&capsule)?;
    let mut repaired_paths = Vec::new();
    let mut overwritten_paths = Vec::new();
    let mut skipped_paths = Vec::new();

    for file in files {
        let path = workspace_root.join(&file.relative_path);
        let existed = path.exists();

        if existed && !options.force {
            skipped_paths.push(relative_display(&file.relative_path));
            continue;
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create parent directory for {}",
                    display_path(&path)
                )
            })?;
        }

        fs::write(&path, file.body)
            .with_context(|| format!("failed to write {}", display_path(&path)))?;

        if existed {
            overwritten_paths.push(relative_display(&file.relative_path));
        } else {
            repaired_paths.push(relative_display(&file.relative_path));
        }
    }

    Ok(ProjectRepairResult {
        ok: true,
        status: if overwritten_paths.is_empty() {
            "repaired".to_string()
        } else {
            "repaired_with_overwrites".to_string()
        },
        workspace_root: display_path(&workspace_root),
        repaired_paths,
        overwritten_paths,
        skipped_paths,
    })
}

pub fn preview_plan(options: ProjectPreviewPlanOptions) -> Result<ProjectPreviewPlanResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;
    let command = command_for_kind(&capsule.commands, capsule.preview.command_kind)?;

    if capsule.preview.url.trim().is_empty() {
        bail!("project preview URL is not configured");
    }

    Ok(ProjectPreviewPlanResult {
        ok: true,
        status: "ready".to_string(),
        workspace_root: display_path(&workspace_root),
        stack: capsule.stack,
        url: capsule.preview.url,
        command_kind: capsule.preview.command_kind,
        command,
        readiness_timeout_seconds: capsule.preview.readiness_timeout_seconds,
    })
}

pub fn preview_start(
    options: ProjectPreviewLifecycleOptions,
) -> Result<ProjectPreviewLifecycleResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let plan = preview_plan(ProjectPreviewPlanOptions {
        workspace_root: workspace_root.clone(),
    })?;
    let state_path = preview_state_path(&workspace_root);
    let log_path = preview_log_path(&workspace_root);

    if let Some(state) = load_preview_state_if_present(&state_path)?
        && process_running(state.pid)
    {
        let ready = preview_ready(&state.url);
        return Ok(preview_lifecycle_result(PreviewLifecycleInput {
            status: "already_running",
            pid: Some(state.pid),
            running: true,
            ready,
            url: state.url,
            command: state.command,
            state_path,
            log_path,
        }));
    }

    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(&log_path)
            )
        })?;
    }

    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("failed to open {}", display_path(&log_path)))?;
    let stderr = stdout
        .try_clone()
        .with_context(|| format!("failed to clone {}", display_path(&log_path)))?;
    let child = bash_command()
        .arg("-lc")
        .arg(&plan.command)
        .current_dir(&workspace_root)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .with_context(|| format!("failed to start preview command `{}`", plan.command))?;

    let state = ProjectPreviewState {
        pid: child.id(),
        url: plan.url.clone(),
        command_kind: plan.command_kind,
        command: plan.command.clone(),
        started_at: now_rfc3339()?,
        state_path: display_path(&state_path),
        log_path: display_path(&log_path),
    };
    write_json(&state_path, &state)?;

    let ready = wait_for_preview_ready(&state.url, plan.readiness_timeout_seconds);
    let running = process_running(state.pid);
    let status = if running && ready {
        "running_ready"
    } else if running {
        "running_not_ready"
    } else {
        "exited"
    };

    Ok(preview_lifecycle_result(PreviewLifecycleInput {
        status,
        pid: Some(state.pid),
        running,
        ready,
        url: state.url,
        command: state.command,
        state_path,
        log_path,
    }))
}

pub fn preview_status(
    options: ProjectPreviewLifecycleOptions,
) -> Result<ProjectPreviewLifecycleResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let state_path = preview_state_path(&workspace_root);
    let log_path = preview_log_path(&workspace_root);

    let Some(state) = load_preview_state_if_present(&state_path)? else {
        return Ok(preview_lifecycle_result(PreviewLifecycleInput {
            status: "stopped",
            pid: None,
            running: false,
            ready: false,
            url: String::new(),
            command: String::new(),
            state_path,
            log_path,
        }));
    };

    let running = process_running(state.pid);
    let ready = running && preview_ready(&state.url);
    let status = if running && ready {
        "running_ready"
    } else if running {
        "running_not_ready"
    } else {
        "exited"
    };

    Ok(preview_lifecycle_result(PreviewLifecycleInput {
        status,
        pid: Some(state.pid),
        running,
        ready,
        url: state.url,
        command: state.command,
        state_path,
        log_path,
    }))
}

pub fn preview_stop(
    options: ProjectPreviewLifecycleOptions,
) -> Result<ProjectPreviewLifecycleResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let state_path = preview_state_path(&workspace_root);
    let log_path = preview_log_path(&workspace_root);

    let Some(state) = load_preview_state_if_present(&state_path)? else {
        return Ok(preview_lifecycle_result(PreviewLifecycleInput {
            status: "stopped",
            pid: None,
            running: false,
            ready: false,
            url: String::new(),
            command: String::new(),
            state_path,
            log_path,
        }));
    };

    if process_running(state.pid) {
        let pid = Pid::from_u32(state.pid);
        let mut sys = System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        if let Some(process) = sys.process(pid) {
            process.kill();
        }
        thread::sleep(Duration::from_millis(100));
    }

    let running = process_running(state.pid);
    if !running {
        let _ = fs::remove_file(&state_path);
    }

    let status = if running { "stop_requested" } else { "stopped" };

    Ok(preview_lifecycle_result(PreviewLifecycleInput {
        status,
        pid: Some(state.pid),
        running,
        ready: false,
        url: state.url,
        command: state.command,
        state_path,
        log_path,
    }))
}

pub fn preview_check(options: ProjectPreviewCheckOptions) -> Result<ProjectPreviewCheckResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let state_path = preview_state_path(&workspace_root);
    let log_path = preview_log_path(&workspace_root);

    if let Some(state) = load_preview_state_if_present(&state_path)? {
        let running = process_running(state.pid);
        let mode = preview_mode(&state.url);
        let ready = if mode == "native" {
            running
        } else {
            running && preview_ready(&state.url)
        };
        let status = if ready {
            "ready"
        } else if running {
            "not_ready"
        } else {
            "exited"
        };
        let detail = if ready {
            "preview target is ready"
        } else if running {
            "preview process is running but target is not reachable"
        } else {
            "preview process is not running"
        };

        return Ok(ProjectPreviewCheckResult {
            ok: ready,
            status: status.to_string(),
            mode: mode.to_string(),
            pid: Some(state.pid),
            running,
            ready,
            url: state.url,
            command: state.command,
            state_path: display_path(&state_path),
            log_path: display_path(&log_path),
            detail: detail.to_string(),
        });
    }

    let plan = preview_plan(ProjectPreviewPlanOptions {
        workspace_root: workspace_root.clone(),
    })?;
    let mode = preview_mode(&plan.url);
    let ready = mode == "web" && preview_ready(&plan.url);
    let status = if ready {
        "reachable_without_state"
    } else {
        "stopped"
    };
    let detail = if ready {
        "preview target is reachable but no managed preview state exists"
    } else {
        "no managed preview state exists"
    };

    Ok(ProjectPreviewCheckResult {
        ok: ready,
        status: status.to_string(),
        mode: mode.to_string(),
        pid: None,
        running: false,
        ready,
        url: plan.url,
        command: plan.command,
        state_path: display_path(&state_path),
        log_path: display_path(&log_path),
        detail: detail.to_string(),
    })
}

pub fn run_project_command(options: ProjectRunCommandOptions) -> Result<ProjectRunCommandResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;
    let command = command_for_kind(&capsule.commands, options.kind)?;
    let build_log_path = workspace_root.join(&capsule.state.build_log);

    if options.dry_run {
        return Ok(ProjectRunCommandResult {
            ok: true,
            status: "dry_run".to_string(),
            workspace_root: display_path(&workspace_root),
            command_kind: options.kind,
            command,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            build_log_path: display_path(&build_log_path),
        });
    }

    let output = bash_command()
        .arg("-lc")
        .arg(&command)
        .current_dir(&workspace_root)
        .output()
        .with_context(|| format!("failed to run project command `{command}`"))?;

    let exit_code = output.status.code();
    let ok = output.status.success();
    let status = if ok { "completed" } else { "failed" }.to_string();
    append_command_log(&build_log_path, options.kind, &command, &status, exit_code)?;

    Ok(ProjectRunCommandResult {
        ok,
        status,
        workspace_root: display_path(&workspace_root),
        command_kind: options.kind,
        command,
        exit_code,
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        build_log_path: display_path(&build_log_path),
    })
}

pub fn verify_generated_project(
    options: ProjectVerifyGeneratedOptions,
) -> Result<ProjectVerifyGeneratedResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let mut steps = Vec::new();

    let inspect = inspect_project(ProjectInspectOptions {
        workspace_root: workspace_root.clone(),
    });
    if !push_inspect_step(&mut steps, inspect) {
        return Ok(verify_generated_result(workspace_root, steps));
    }

    let doctor = doctor_project(ProjectDoctorOptions {
        workspace_root: workspace_root.clone(),
    });
    if !push_doctor_step(&mut steps, doctor) {
        return Ok(verify_generated_result(workspace_root, steps));
    }

    for (name, kind) in [
        ("setup", ProjectCommandKind::Setup),
        ("test", ProjectCommandKind::Test),
        ("build", ProjectCommandKind::Build),
    ] {
        let command_result = run_project_command(ProjectRunCommandOptions {
            workspace_root: workspace_root.clone(),
            kind,
            dry_run: false,
        });

        if !push_command_step(&mut steps, name, command_result) {
            return Ok(verify_generated_result(workspace_root, steps));
        }
    }

    let preview_started = push_preview_lifecycle_step(
        &mut steps,
        "preview_start",
        preview_start(ProjectPreviewLifecycleOptions {
            workspace_root: workspace_root.clone(),
        }),
    );

    if preview_started {
        push_preview_check_step(
            &mut steps,
            preview_check(ProjectPreviewCheckOptions {
                workspace_root: workspace_root.clone(),
            }),
        );
    }

    if preview_started {
        push_preview_lifecycle_step(
            &mut steps,
            "preview_stop",
            preview_stop(ProjectPreviewLifecycleOptions {
                workspace_root: workspace_root.clone(),
            }),
        );
    }

    Ok(verify_generated_result(workspace_root, steps))
}

pub fn doctor_project(options: ProjectDoctorOptions) -> Result<ProjectDoctorResult> {
    let workspace_root = absolute_path(&options.workspace_root)?;
    let capsule_path = workspace_root.join(".mutagen/project.json");
    let capsule = load_capsule(&capsule_path)?;
    let requirements = toolchain_requirements(&capsule.stack)?;
    let checks = requirements
        .iter()
        .map(|executable| doctor_check(executable))
        .collect::<Vec<_>>();
    let ok = checks.iter().all(|check| check.ok);

    Ok(ProjectDoctorResult {
        ok,
        status: if ok { "ready" } else { "missing_tools" }.to_string(),
        workspace_root: display_path(&workspace_root),
        stack: capsule.stack,
        checks,
    })
}

fn command_for_kind(commands: &ProjectCommands, kind: ProjectCommandKind) -> Result<String> {
    let command = match kind {
        ProjectCommandKind::Setup => &commands.setup,
        ProjectCommandKind::Dev => &commands.dev,
        ProjectCommandKind::Test => &commands.test,
        ProjectCommandKind::Build => &commands.build,
    };

    if command.trim().is_empty() {
        bail!(
            "project command `{}` is not configured",
            command_kind_name(kind)
        );
    }

    Ok(command.clone())
}

fn toolchain_requirements(stack: &str) -> Result<Vec<&'static str>> {
    match stack {
        "nextjs-postgres" | "vite-express-sqlite" | "cloudflare-worker" => Ok(vec!["node", "npm"]),
        "fastapi-react" => Ok(vec!["python", "npm"]),
        "aspnet-blazor" => Ok(vec!["dotnet"]),
        "rust-bevy" => Ok(vec!["cargo", "rustc"]),
        stack => bail!("unsupported stack `{stack}` for doctor checks"),
    }
}

fn doctor_check(executable: &str) -> ProjectDoctorCheck {
    let output = Command::new(executable).arg("--version").output();

    match output {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            ProjectDoctorCheck {
                executable: executable.to_string(),
                ok: true,
                status: "found".to_string(),
                detail: if version.is_empty() {
                    format!("`{executable}` is available")
                } else {
                    version
                },
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            ProjectDoctorCheck {
                executable: executable.to_string(),
                ok: false,
                status: "failed".to_string(),
                detail: if stderr.is_empty() {
                    format!("`{executable} --version` exited unsuccessfully")
                } else {
                    stderr
                },
            }
        }
        Err(error) => ProjectDoctorCheck {
            executable: executable.to_string(),
            ok: false,
            status: "missing".to_string(),
            detail: error.to_string(),
        },
    }
}

fn verify_generated_result(
    workspace_root: PathBuf,
    steps: Vec<ProjectVerifyGeneratedStep>,
) -> ProjectVerifyGeneratedResult {
    let ok = steps.iter().all(|step| step.ok);

    ProjectVerifyGeneratedResult {
        ok,
        status: if ok { "verified" } else { "failed" }.to_string(),
        workspace_root: display_path(&workspace_root),
        steps,
    }
}

fn push_inspect_step(
    steps: &mut Vec<ProjectVerifyGeneratedStep>,
    result: Result<ProjectInspectResult>,
) -> bool {
    match result {
        Ok(result) => {
            let detail = if result.missing_paths.is_empty() {
                "project capsule and required artifacts are present".to_string()
            } else {
                format!("missing paths: {}", result.missing_paths.join(", "))
            };
            steps.push(ProjectVerifyGeneratedStep {
                name: "inspect".to_string(),
                ok: result.ok,
                status: result.status,
                detail,
            });
            result.ok
        }
        Err(error) => {
            steps.push(ProjectVerifyGeneratedStep {
                name: "inspect".to_string(),
                ok: false,
                status: "error".to_string(),
                detail: error.to_string(),
            });
            false
        }
    }
}

fn push_doctor_step(
    steps: &mut Vec<ProjectVerifyGeneratedStep>,
    result: Result<ProjectDoctorResult>,
) -> bool {
    match result {
        Ok(result) => {
            let detail = result
                .checks
                .iter()
                .map(|check| format!("{}: {} ({})", check.executable, check.status, check.detail))
                .collect::<Vec<_>>()
                .join("; ");
            steps.push(ProjectVerifyGeneratedStep {
                name: "doctor".to_string(),
                ok: result.ok,
                status: result.status,
                detail,
            });
            result.ok
        }
        Err(error) => {
            steps.push(ProjectVerifyGeneratedStep {
                name: "doctor".to_string(),
                ok: false,
                status: "error".to_string(),
                detail: error.to_string(),
            });
            false
        }
    }
}

fn push_command_step(
    steps: &mut Vec<ProjectVerifyGeneratedStep>,
    name: &str,
    result: Result<ProjectRunCommandResult>,
) -> bool {
    match result {
        Ok(result) => {
            let detail = match result.exit_code {
                Some(code) => format!("command `{}` exited with {code}", result.command),
                None => format!("command `{}` completed without exit code", result.command),
            };
            steps.push(ProjectVerifyGeneratedStep {
                name: name.to_string(),
                ok: result.ok,
                status: result.status,
                detail,
            });
            result.ok
        }
        Err(error) => {
            steps.push(ProjectVerifyGeneratedStep {
                name: name.to_string(),
                ok: false,
                status: "error".to_string(),
                detail: error.to_string(),
            });
            false
        }
    }
}

fn push_preview_lifecycle_step(
    steps: &mut Vec<ProjectVerifyGeneratedStep>,
    name: &str,
    result: Result<ProjectPreviewLifecycleResult>,
) -> bool {
    match result {
        Ok(result) => {
            let detail = if result.pid.is_some() {
                format!(
                    "preview `{}` at `{}`; running={}, ready={}",
                    result.command, result.url, result.running, result.ready
                )
            } else {
                "no managed preview process".to_string()
            };
            steps.push(ProjectVerifyGeneratedStep {
                name: name.to_string(),
                ok: result.ok,
                status: result.status,
                detail,
            });
            result.ok
        }
        Err(error) => {
            steps.push(ProjectVerifyGeneratedStep {
                name: name.to_string(),
                ok: false,
                status: "error".to_string(),
                detail: error.to_string(),
            });
            false
        }
    }
}

fn push_preview_check_step(
    steps: &mut Vec<ProjectVerifyGeneratedStep>,
    result: Result<ProjectPreviewCheckResult>,
) -> bool {
    match result {
        Ok(result) => {
            let ok = result.ok;
            steps.push(ProjectVerifyGeneratedStep {
                name: "preview_check".to_string(),
                ok,
                status: result.status,
                detail: result.detail,
            });
            ok
        }
        Err(error) => {
            steps.push(ProjectVerifyGeneratedStep {
                name: "preview_check".to_string(),
                ok: false,
                status: "error".to_string(),
                detail: error.to_string(),
            });
            false
        }
    }
}

fn append_command_log(
    path: &Path,
    kind: ProjectCommandKind,
    command: &str,
    status: &str,
    exit_code: Option<i32>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(path)
            )
        })?;
    }

    let entry = ProjectCommandLogEntry {
        event: "project_command".to_string(),
        command_kind: kind,
        command: command.to_string(),
        status: status.to_string(),
        exit_code,
        recorded_at: now_rfc3339()?,
    };
    let line = serde_json::to_string(&entry).context("failed to serialize project command log")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", display_path(path)))?;
    writeln!(file, "{line}").with_context(|| format!("failed to write {}", display_path(path)))
}

fn write_feature_brief(workspace_root: &Path, feature: &ProjectFeatureIntent) -> Result<()> {
    let path = workspace_root.join(&feature.brief_path);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(&path)
            )
        })?;
    }

    let body = format!(
        "# {}\n\nStatus: {}\nTarget stack: {}\nCreated: {}\n\n## Request\n\n{}\n",
        feature.title,
        feature.status,
        feature.target_stack,
        feature.created_at,
        if feature.description.is_empty() {
            "_No description provided._"
        } else {
            &feature.description
        }
    );

    fs::write(&path, body).with_context(|| format!("failed to write {}", display_path(&path)))
}

fn write_project_brief_intake(brief_path: &Path, title: &str, prompt: &str) -> Result<()> {
    if let Some(parent) = brief_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(brief_path)
            )
        })?;
    }

    let existing = if brief_path.exists() {
        fs::read_to_string(brief_path)
            .with_context(|| format!("failed to read {}", display_path(brief_path)))?
    } else {
        "# Design Brief\n".to_string()
    };

    let timestamp = now_rfc3339()?;
    let current_direction = format!("{prompt}\n");
    let intake_entry = format!("### {timestamp} - {title}\n\n{prompt}\n");
    let updated = append_project_brief_intake(&existing, &current_direction, &intake_entry);

    fs::write(brief_path, updated)
        .with_context(|| format!("failed to write {}", display_path(brief_path)))
}

fn append_project_brief_intake(
    existing: &str,
    current_direction: &str,
    intake_entry: &str,
) -> String {
    let mut body = if existing.trim().is_empty() {
        "# Design Brief\n".to_string()
    } else {
        existing.to_string()
    };

    if !body.starts_with("# Design Brief") {
        body = format!("# Design Brief\n\n{}", body.trim_start());
    }

    let updated = upsert_markdown_section(&body, "Current direction", current_direction);
    append_markdown_section_entry(&updated, "Intake log", intake_entry)
}

fn load_project_brief_summary(brief_path: &Path) -> Result<ProjectDesignBriefSummary> {
    if !brief_path.exists() {
        return Ok(ProjectDesignBriefSummary {
            path: display_path(brief_path),
            exists: false,
            excerpt: String::new(),
        });
    }

    let raw = fs::read_to_string(brief_path)
        .with_context(|| format!("failed to read {}", display_path(brief_path)))?;
    let excerpt = project_brief_excerpt(&raw);

    Ok(ProjectDesignBriefSummary {
        path: display_path(brief_path),
        exists: true,
        excerpt,
    })
}

fn project_brief_excerpt(raw: &str) -> String {
    if let Some(section) = markdown_section_body(raw, "Current direction") {
        let excerpt = truncate_text(&normalize_text(&section), 240);
        if !excerpt.is_empty() {
            return excerpt;
        }
    }

    truncate_text(
        &normalize_text(
            &raw.lines()
                .filter(|line| !line.trim_start().starts_with('#'))
                .collect::<Vec<_>>()
                .join(" "),
        ),
        240,
    )
}

fn derive_project_intake_title(prompt: &str) -> String {
    let first_line = prompt
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Project request");
    let sentence = first_line
        .split_terminator(['.', '!', '?'])
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(first_line);
    let normalized = normalize_text(sentence);

    if normalized.is_empty() {
        "Project request".to_string()
    } else {
        truncate_text(&normalized, 72)
    }
}

fn append_feature_log(path: &Path, feature: &ProjectFeatureIntent) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(path)
            )
        })?;
    }

    let line = serde_json::to_string(feature).context("failed to serialize feature intent")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", display_path(path)))?;
    writeln!(file, "{line}").with_context(|| format!("failed to write {}", display_path(path)))
}

fn read_feature_log(path: &Path) -> Result<Vec<ProjectFeatureIntent>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read feature log at {}", display_path(path)))?;
    raw.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str(line).with_context(|| {
                format!(
                    "failed to parse feature log entry {} at {}",
                    index + 1,
                    display_path(path)
                )
            })
        })
        .collect()
}

fn feature_plan(feature: &ProjectFeatureIntent, plan_path: String) -> Result<ProjectFeaturePlan> {
    let (target_paths, verification_commands) = feature_plan_stack_defaults(&feature.target_stack)?;
    let title = feature.title.clone();

    Ok(ProjectFeaturePlan {
        feature_id: feature.id.clone(),
        title: title.clone(),
        status: "planned".to_string(),
        target_stack: feature.target_stack.clone(),
        plan_path,
        generated_at: now_rfc3339()?,
        summary: if feature.description.is_empty() {
            format!("Plan implementation work for {title}.")
        } else {
            feature.description.clone()
        },
        target_paths,
        verification_commands,
        steps: vec![
            ProjectFeaturePlanStep {
                id: "understand".to_string(),
                title: "Clarify behavior".to_string(),
                description: "Confirm the user-visible behavior, data shape, and acceptance criteria for the feature.".to_string(),
            },
            ProjectFeaturePlanStep {
                id: "implement".to_string(),
                title: "Implement scoped changes".to_string(),
                description: "Change only the stack-specific files needed for the feature and preserve the generated project structure.".to_string(),
            },
            ProjectFeaturePlanStep {
                id: "verify".to_string(),
                title: "Verify generated project".to_string(),
                description: "Run the configured test/build/preview checks before marking the feature complete.".to_string(),
            },
        ],
    })
}

fn load_feature_plan(path: &Path) -> Result<ProjectFeaturePlan> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read feature plan at {}", display_path(path)))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse feature plan at {}", display_path(path)))
}

fn load_feature_slice_manifest(path: &Path) -> Result<ProjectFeatureSliceManifest> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read feature slices at {}", display_path(path)))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse feature slices at {}", display_path(path)))
}

fn feature_slice_manifest(
    feature: &ProjectFeatureIntent,
    plan: &ProjectFeaturePlan,
) -> Result<ProjectFeatureSliceManifest> {
    let slices_path = format!(".mutagen/features/{}/slices.json", feature.id);
    let slices = plan
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| ProjectFeatureSlice {
            id: format!("slice-{:03}-{}", index + 1, package_name(&step.id)),
            title: step.title.clone(),
            status: "queued".to_string(),
            plan_step_id: step.id.clone(),
            summary: step.description.clone(),
            target_paths: plan.target_paths.clone(),
            verification_commands: plan.verification_commands.clone(),
        })
        .collect();

    Ok(ProjectFeatureSliceManifest {
        schema_version: 1,
        feature_id: feature.id.clone(),
        title: feature.title.clone(),
        status: "sliced".to_string(),
        target_stack: feature.target_stack.clone(),
        source_plan_path: plan.plan_path.clone(),
        slices_path,
        generated_at: now_rfc3339()?,
        slices,
    })
}

fn feature_queue_slices(
    feature: &ProjectFeatureIntent,
    manifest: &ProjectFeatureSliceManifest,
) -> Vec<Slice> {
    let mut previous_id: Option<String> = None;
    let bounded_context = package_name(&feature.title);
    let mut queue_slices = Vec::new();

    for feature_slice in &manifest.slices {
        let id = format!("{}-{}", feature.id, feature_slice.id);
        let depends_on = previous_id.iter().cloned().collect::<Vec<_>>();
        previous_id = Some(id.clone());

        queue_slices.push(Slice {
            id,
            title: format!("{}: {}", feature.title, feature_slice.title),
            phase: Some("feature".to_string()),
            status: SliceStatus::Pending,
            author_agent: "Bebop".to_string(),
            layer: 1,
            bounded_context: bounded_context.clone(),
            target_loc: 150,
            objective: feature_slice.summary.clone(),
            context_to_update: "project_state.md".to_string(),
            implementation_details: feature_implementation_details(feature_slice),
            review_required: true,
            attempts: 0,
            micro_corrections_used: 0,
            depends_on,
            adjacent_scope_allowed: Vec::new(),
            write_set: feature_slice.target_paths.clone(),
            traces_to: TraceSet {
                prd: vec![feature.id.clone()],
                adr: Vec::new(),
                ddd: Vec::new(),
                isc: Vec::new(),
                dsd: Vec::new(),
            },
            verification_steps: VerificationSteps {
                acceptance: feature_acceptance_text(feature_slice),
                isc_detection: "Confirm the feature does not introduce undocumented integration or data-contract drift.".to_string(),
                dsd_conformance: "Confirm UI changes preserve the generated project's design system.".to_string(),
            },
            human_check_needed: HumanCheckNeeded::default(),
            verdicts: Default::default(),
            completed_at: None,
            escalation_reason: None,
        });
    }

    queue_slices
}

fn next_feature_slice_id(slices: &[Slice], feature_id: &str) -> Option<String> {
    let prefix = format!("{feature_id}-");

    slices
        .iter()
        .find(|slice| slice.id.starts_with(&prefix) && slice.status != SliceStatus::Completed)
        .map(|slice| slice.id.clone())
}

fn feature_slices(slices: &[Slice], feature_id: &str) -> Vec<Slice> {
    let prefix = format!("{feature_id}-");

    slices
        .iter()
        .filter(|slice| slice.id.starts_with(&prefix))
        .cloned()
        .collect()
}

fn feature_progress_counts(slices: &[Slice]) -> ProjectFeatureProgressCounts {
    let mut counts = ProjectFeatureProgressCounts::default();

    for slice in slices {
        match slice.status {
            SliceStatus::Pending => counts.pending += 1,
            SliceStatus::InProgress => counts.in_progress += 1,
            SliceStatus::BlockedRetry => counts.blocked_retry += 1,
            SliceStatus::Completed => counts.completed += 1,
            SliceStatus::Escalated => counts.escalated += 1,
            SliceStatus::Refused => counts.refused += 1,
            // FinalizationFailed parks the slice the same way refusal does
            // until the operator either re-runs author or overrides; bucket
            // it with refused so progress views surface it as "needs attention".
            SliceStatus::FinalizationFailed => counts.refused += 1,
        }
    }

    counts
}

fn feature_progress_status(total: usize, counts: &ProjectFeatureProgressCounts) -> String {
    if total == 0 {
        return "not_enqueued".to_string();
    }

    if counts.escalated > 0 || counts.refused > 0 {
        return "attention".to_string();
    }

    if counts.completed == total {
        return "complete".to_string();
    }

    if counts.in_progress > 0 {
        return "in_progress".to_string();
    }

    if counts.blocked_retry > 0 {
        return "blocked_retry".to_string();
    }

    "queued".to_string()
}

fn load_feature_active_slice(
    active_state_path: &Path,
    feature_id: &str,
) -> Result<Option<ProjectFeatureActiveSlice>> {
    if !active_state_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(active_state_path)
        .with_context(|| format!("failed to read {}", display_path(active_state_path)))?;
    let active_state: ActiveSliceState = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", display_path(active_state_path)))?;
    let prefix = format!("{feature_id}-");

    if !active_state.slice_id.starts_with(&prefix) {
        return Ok(None);
    }

    Ok(Some(ProjectFeatureActiveSlice {
        id: active_state.slice_id,
        title: active_state.title,
        stage: format!("{:?}", active_state.stage).to_ascii_lowercase(),
        active_agent: active_state.active_agent,
        host: active_state.host,
        evidence_bundle_path: active_state.evidence_bundle_path,
    }))
}

fn active_feature_progress(workspace_root: &Path) -> Result<Option<ProjectFeatureProgressResult>> {
    let active_state_path = workspace_root.join(".mutagen/state/active-slice.json");

    if !active_state_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&active_state_path)
        .with_context(|| format!("failed to read {}", display_path(&active_state_path)))?;
    let active_state: ActiveSliceState = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", display_path(&active_state_path)))?;
    let feature_id = active_state
        .slice_id
        .split("-slice-")
        .next()
        .filter(|value| value.starts_with("feature-"))
        .map(str::to_string);

    let Some(feature_id) = feature_id else {
        return Ok(None);
    };

    feature_progress(ProjectFeatureProgressOptions {
        workspace_root: workspace_root.to_path_buf(),
        feature_id,
    })
    .map(Some)
}

fn dashboard_feature_backlog(
    workspace_root: &Path,
    features: Vec<ProjectFeatureIntent>,
) -> Result<ProjectDashboardFeatureBacklog> {
    let mut items = Vec::with_capacity(features.len());
    let mut queued = 0;
    let mut planned = 0;
    let mut ready = 0;
    let mut in_queue = 0;

    for feature in features {
        let derived_status = dashboard_feature_status(workspace_root, &feature);
        match derived_status.as_str() {
            "queued" => queued += 1,
            "planned" => planned += 1,
            "ready" => ready += 1,
            "in_queue" => in_queue += 1,
            _ => {}
        }

        items.push(ProjectDashboardFeatureItem {
            id: feature.id,
            title: feature.title,
            status: derived_status,
            created_at: feature.created_at,
        });
    }

    Ok(ProjectDashboardFeatureBacklog {
        total: items.len(),
        queued,
        planned,
        ready,
        in_queue,
        features: items,
    })
}

fn dashboard_feature_status(workspace_root: &Path, feature: &ProjectFeatureIntent) -> String {
    let feature_root = workspace_root.join(".mutagen/features").join(&feature.id);
    let plan_exists = feature_root.join("plan.json").exists();
    let slices_exists = feature_root.join("slices.json").exists();
    let in_queue = workspace_root.join("slices/queue.json").exists()
        && feature_queue_membership_exists(workspace_root, &feature.id);

    if in_queue {
        "in_queue".to_string()
    } else if plan_exists && slices_exists {
        "ready".to_string()
    } else if plan_exists {
        "planned".to_string()
    } else {
        "queued".to_string()
    }
}

fn feature_queue_membership_exists(workspace_root: &Path, feature_id: &str) -> bool {
    let queue_path = workspace_root.join("slices/queue.json");
    let Ok(queue) = load_queue_file(&queue_path) else {
        return false;
    };

    let prefix = format!("{feature_id}-");
    queue
        .slices
        .iter()
        .any(|slice| slice.id.starts_with(&prefix))
}

fn feature_implementation_details(feature_slice: &ProjectFeatureSlice) -> Vec<String> {
    let mut details = vec![feature_slice.summary.clone()];

    if !feature_slice.target_paths.is_empty() {
        details.push(format!(
            "Target paths: {}",
            feature_slice.target_paths.join(", ")
        ));
    }

    if !feature_slice.verification_commands.is_empty() {
        details.push(format!(
            "Verification commands: {}",
            feature_slice.verification_commands.join("; ")
        ));
    }

    details
}

fn feature_acceptance_text(feature_slice: &ProjectFeatureSlice) -> String {
    if feature_slice.verification_commands.is_empty() {
        return "Run the generated project's configured verification commands.".to_string();
    }

    format!(
        "Run and pass: {}",
        feature_slice.verification_commands.join("; ")
    )
}

fn ensure_feature_prd_section(
    prd_path: &Path,
    feature: &ProjectFeatureIntent,
    manifest: &ProjectFeatureSliceManifest,
) -> Result<()> {
    let existing = if prd_path.exists() {
        fs::read_to_string(prd_path)
            .with_context(|| format!("failed to read {}", display_path(prd_path)))?
    } else {
        String::new()
    };

    if existing.contains(&feature.id) {
        return Ok(());
    }

    if let Some(parent) = prd_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(prd_path)
            )
        })?;
    }

    let mut body = existing.trim_end().to_string();
    if body.is_empty() {
        body.push_str("# Product Requirements Document");
    }

    body.push_str(&format!(
        "\n\n## Feature: {} ({})\n\nStatus: {}\nTarget stack: {}\n\n{}\n\n### Execution slices\n\n{}\n",
        feature.title,
        feature.id,
        manifest.status,
        feature.target_stack,
        if feature.description.is_empty() {
            "No additional description was provided."
        } else {
            &feature.description
        },
        manifest
            .slices
            .iter()
            .map(|slice| format!("- {}: {}", slice.id, slice.summary))
            .collect::<Vec<_>>()
            .join("\n")
    ));

    fs::write(prd_path, body).with_context(|| format!("failed to write {}", display_path(prd_path)))
}

fn feature_plan_stack_defaults(stack: &str) -> Result<(Vec<String>, Vec<String>)> {
    match stack {
        "vite-express-sqlite" => Ok((
            vec![
                "src/App.jsx".to_string(),
                "src/styles.css".to_string(),
                "server/index.js".to_string(),
                "server/db.js".to_string(),
                "server/db.test.js".to_string(),
            ],
            vec![
                "npm test".to_string(),
                "npm run build".to_string(),
                "bash plugins/mutagen/scripts/project.sh verify-generated".to_string(),
            ],
        )),
        "rust-bevy" => Ok((
            vec!["src/main.rs".to_string(), "Cargo.toml".to_string()],
            vec![
                "cargo test".to_string(),
                "cargo build --release".to_string(),
                "bash plugins/mutagen/scripts/project.sh verify-generated".to_string(),
            ],
        )),
        "nextjs-postgres" => Ok((
            vec![
                "app/**".to_string(),
                "components/**".to_string(),
                "lib/**".to_string(),
                "tests/**".to_string(),
            ],
            vec![
                "npm test".to_string(),
                "npm run build".to_string(),
                "bash plugins/mutagen/scripts/project.sh verify-generated".to_string(),
            ],
        )),
        "fastapi-react" => Ok((
            vec![
                "src/**".to_string(),
                "app/**".to_string(),
                "tests/**".to_string(),
            ],
            vec![
                "python -m pytest".to_string(),
                "npm test".to_string(),
                "bash plugins/mutagen/scripts/project.sh verify-generated".to_string(),
            ],
        )),
        "aspnet-blazor" => Ok((
            vec![
                "Pages/**".to_string(),
                "Components/**".to_string(),
                "Services/**".to_string(),
                "Tests/**".to_string(),
            ],
            vec![
                "dotnet test".to_string(),
                "dotnet build".to_string(),
                "bash plugins/mutagen/scripts/project.sh verify-generated".to_string(),
            ],
        )),
        "cloudflare-worker" => Ok((
            vec!["src/**".to_string(), "test/**".to_string()],
            vec![
                "npm test".to_string(),
                "npm run build".to_string(),
                "bash plugins/mutagen/scripts/project.sh verify-generated".to_string(),
            ],
        )),
        stack => bail!("unsupported stack `{stack}` for feature planning"),
    }
}

fn last_build_log_entry(path: &Path) -> Result<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read build log at {}", display_path(path)))?;
    let Some(line) = raw.lines().rev().find(|line| !line.trim().is_empty()) else {
        return Ok(None);
    };
    let value = serde_json::from_str(line)
        .with_context(|| format!("failed to parse build log entry at {}", display_path(path)))?;

    Ok(Some(value))
}

fn now_rfc3339() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format current timestamp")
}

fn command_kind_name(kind: ProjectCommandKind) -> &'static str {
    match kind {
        ProjectCommandKind::Setup => "setup",
        ProjectCommandKind::Dev => "dev",
        ProjectCommandKind::Test => "test",
        ProjectCommandKind::Build => "build",
    }
}

fn preview_state_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".mutagen/state/preview.json")
}

fn preview_log_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".mutagen/state/preview-output.log")
}

fn load_preview_state_if_present(path: &Path) -> Result<Option<ProjectPreviewState>> {
    if !path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read preview state at {}", display_path(path)))?;
    let state = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse preview state at {}", display_path(path)))?;
    Ok(Some(state))
}

/// Argument bundle for `preview_lifecycle_result`. Bundles the eight inputs the
/// six call sites used to pass positionally; named fields make the boolean
/// flags self-documenting and keep the function under the clippy
/// `too_many_arguments` threshold.
struct PreviewLifecycleInput {
    status: &'static str,
    pid: Option<u32>,
    running: bool,
    ready: bool,
    url: String,
    command: String,
    state_path: PathBuf,
    log_path: PathBuf,
}

fn preview_lifecycle_result(input: PreviewLifecycleInput) -> ProjectPreviewLifecycleResult {
    let PreviewLifecycleInput {
        status,
        pid,
        running,
        ready,
        url,
        command,
        state_path,
        log_path,
    } = input;
    ProjectPreviewLifecycleResult {
        ok: matches!(status, "already_running" | "running_ready" | "stopped"),
        status: status.to_string(),
        pid,
        running,
        ready,
        url,
        command,
        state_path: display_path(&state_path),
        log_path: display_path(&log_path),
    }
}

/// Bash on Windows is a minefield. A bare `bash` lookup grabs whatever lands
/// first on PATH, and on GitHub's windows-latest runner that's the System32 WSL
/// stub — which, with no distro installed, just exits 1 and drags every project
/// command down with it. Git for Windows ships the bash we actually want, so go
/// hunt that down first and only fall back to a bare `bash` when it's nowhere to
/// be found (notably every non-Windows host, where bare `bash` is exactly right).
fn bash_command() -> Command {
    Command::new(resolve_bash())
}

#[cfg(not(windows))]
fn resolve_bash() -> std::ffi::OsString {
    std::ffi::OsString::from("bash")
}

#[cfg(windows)]
fn resolve_bash() -> std::ffi::OsString {
    git_bash_path()
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|| std::ffi::OsString::from("bash"))
}

#[cfg(windows)]
fn git_bash_path() -> Option<PathBuf> {
    let mut git_roots: Vec<PathBuf> = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var) {
            git_roots.push(PathBuf::from(dir).join("Git"));
        }
    }
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        git_roots.push(PathBuf::from(dir).join("Programs").join("Git"));
    }
    if let Some(root) = git_root_from_path() {
        git_roots.push(root);
    }

    git_roots.into_iter().find_map(|root| {
        ["bin\\bash.exe", "usr\\bin\\bash.exe"]
            .iter()
            .map(|leaf| root.join(leaf))
            .find(|candidate| candidate.is_file())
    })
}

/// Belt-and-suspenders for installs that don't sit under a standard Program
/// Files dir: walk PATH to find git.exe, then climb back up to the Git root that
/// has a bash next to it.
#[cfg(windows)]
fn git_root_from_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if !dir.join("git.exe").is_file() {
            continue;
        }
        // git.exe lives in <root>\cmd or <root>\mingw64\bin depending on the
        // install, so just climb parents until one looks like the Git root.
        let mut ancestor: Option<&Path> = Some(dir.as_path());
        while let Some(current) = ancestor {
            if current.join("usr\\bin\\bash.exe").is_file()
                || current.join("bin\\bash.exe").is_file()
            {
                return Some(current.to_path_buf());
            }
            ancestor = current.parent();
        }
    }
    None
}

fn process_running(pid: u32) -> bool {
    // Used to shell out to `kill -0` + `ps`, which blew up on Windows: a bash
    // child's `child.id()` is a Windows PID, but Git Bash's kill/ps speak MSYS
    // PIDs, so the lookup never matched and every preview read as dead. sysinfo
    // speaks native PIDs on every platform, so the namespaces finally agree.
    if pid == 0 {
        return false;
    }
    let pid = Pid::from_u32(pid);
    let mut sys = System::new();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    match sys.process(pid) {
        // A reaped-but-uncollected child still shows up as a zombie; it isn't
        // doing anything, so don't count it as a live preview.
        Some(process) => process.status() != ProcessStatus::Zombie,
        None => false,
    }
}

fn wait_for_preview_ready(url: &str, timeout_seconds: u32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(timeout_seconds as u64);
    loop {
        if preview_ready(url) {
            return true;
        }

        if Instant::now() >= deadline {
            return false;
        }

        thread::sleep(Duration::from_millis(200));
    }
}

fn preview_ready(url: &str) -> bool {
    if url.starts_with("native://") {
        return true;
    }

    let Some(address) = preview_socket_address(url) else {
        return false;
    };

    TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok()
}

fn preview_mode(url: &str) -> &'static str {
    if url.starts_with("native://") {
        "native"
    } else {
        "web"
    }
}

fn preview_socket_address(url: &str) -> Option<std::net::SocketAddr> {
    let without_scheme = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))?;
    let authority = without_scheme.split('/').next()?;
    let (host, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        let port = port.parse::<u16>().ok()?;
        (host, port)
    } else if url.starts_with("https://") {
        (authority, 443)
    } else {
        (authority, 80)
    };

    (host, port).to_socket_addrs().ok()?.next()
}

#[derive(Debug, Clone)]
struct ScaffoldFile {
    relative_path: String,
    body: String,
}

fn scaffold_files(capsule: &ProjectCapsule) -> Result<Vec<ScaffoldFile>> {
    match capsule.stack.as_str() {
        "nextjs-postgres" => Ok(nextjs_postgres_files(capsule)),
        "vite-express-sqlite" => Ok(vite_express_sqlite_files(capsule)),
        "fastapi-react" => Ok(fastapi_react_files(capsule)),
        "aspnet-blazor" => Ok(aspnet_blazor_files(capsule)),
        "cloudflare-worker" => Ok(cloudflare_worker_files(capsule)),
        "rust-bevy" => Ok(rust_bevy_files(capsule)),
        stack => bail!(
            "scaffold is not available for stack `{stack}`; run `project blueprints` for supported stacks"
        ),
    }
}

fn scaffold_existing_paths(workspace_root: &Path, files: &[ScaffoldFile]) -> Vec<String> {
    files
        .iter()
        .filter(|file| workspace_root.join(&file.relative_path).exists())
        .map(|file| relative_display(&file.relative_path))
        .collect()
}

fn vite_express_sqlite_files(capsule: &ProjectCapsule) -> Vec<ScaffoldFile> {
    vec![
        scaffold_file(
            "package.json",
            &format!(
                r#"{{
  "name": "{}",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {{
    "dev": "node scripts/dev.mjs",
    "test": "node --test",
    "build": "vite build",
    "start": "node server/index.js"
  }},
  "dependencies": {{
    "@vitejs/plugin-react": "^4.2.1",
    "better-sqlite3": "^9.4.3",
    "express": "^4.18.3",
    "vite": "^5.1.4",
    "react": "^18.2.0",
    "react-dom": "^18.2.0"
  }},
  "devDependencies": {{}}
}}
"#,
                package_name(&capsule.name)
            ),
        ),
        scaffold_file(
            "index.html",
            r#"<div id="root"></div>
<script type="module" src="/src/main.jsx"></script>
"#,
        ),
        scaffold_file(
            "src/main.jsx",
            r#"import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.jsx";
import "./styles.css";

createRoot(document.getElementById("root")).render(<App />);
"#,
        ),
        scaffold_file(
            "src/App.jsx",
            r#"import { useEffect, useState } from "react";

export function App() {
  const [items, setItems] = useState([]);
  const [draft, setDraft] = useState("");

  useEffect(() => {
    fetch("/api/items")
      .then((response) => response.json())
      .then((payload) => setItems(payload.items));
  }, []);

  async function addItem(event) {
    event.preventDefault();
    const name = draft.trim();

    if (!name) {
      return;
    }

    const response = await fetch("/api/items", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });
    const payload = await response.json();
    setItems(payload.items);
    setDraft("");
  }

  return (
    <main className="shell">
      <section className="workspace">
        <p className="eyebrow">Mutagen scaffold</p>
        <h1>Build the useful thing first.</h1>
        <form onSubmit={addItem} className="composer">
          <input
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="Add a working slice"
          />
          <button type="submit">Add</button>
        </form>
        <ul className="items">
          {items.map((item) => (
            <li key={item.id}>{item.name}</li>
          ))}
        </ul>
      </section>
    </main>
  );
}
"#,
        ),
        scaffold_file(
            "src/styles.css",
            r#":root {
  color: #172026;
  background: #eef2f3;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

body {
  margin: 0;
}

button,
input {
  font: inherit;
}

.shell {
  min-height: 100vh;
  display: grid;
  place-items: center;
  padding: 32px;
}

.workspace {
  width: min(760px, 100%);
}

.eyebrow {
  color: #5c6970;
  font-size: 0.78rem;
  font-weight: 700;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

h1 {
  margin: 0 0 24px;
  font-size: clamp(2.4rem, 8vw, 5.5rem);
  line-height: 0.95;
}

.composer {
  display: flex;
  gap: 10px;
  margin-bottom: 18px;
}

.composer input {
  flex: 1;
  min-width: 0;
  border: 1px solid #bac6ca;
  border-radius: 8px;
  padding: 12px 14px;
}

.composer button {
  border: 0;
  border-radius: 8px;
  background: #22313a;
  color: white;
  padding: 0 18px;
  font-weight: 700;
}

.items {
  display: grid;
  gap: 8px;
  padding: 0;
  list-style: none;
}

.items li {
  border: 1px solid #d1dadd;
  border-radius: 8px;
  background: white;
  padding: 12px 14px;
}
"#,
        ),
        scaffold_file(
            "server/db.js",
            r#"import Database from "better-sqlite3";
import fs from "node:fs";
import path from "node:path";

const dataDir = path.resolve("data");
fs.mkdirSync(dataDir, { recursive: true });

export function openDatabase(filename = path.join(dataDir, "app.db")) {
  const database = new Database(filename);
  database.exec(`
    create table if not exists items (
      id integer primary key autoincrement,
      name text not null
    );
  `);
  return database;
}

export function listItems(database) {
  return database.prepare("select id, name from items order by id desc").all();
}

export function createItem(database, name) {
  database.prepare("insert into items (name) values (?)").run(name);
  return listItems(database);
}
"#,
        ),
        scaffold_file(
            "server/index.js",
            r#"import express from "express";
import { createItem, listItems, openDatabase } from "./db.js";

const app = express();
const database = openDatabase();
const port = Number(process.env.PORT || 3001);

app.use(express.json());

app.get("/api/items", (_request, response) => {
  response.json({ items: listItems(database) });
});

app.post("/api/items", (request, response) => {
  const name = String(request.body?.name || "").trim();

  if (!name) {
    response.status(400).json({ error: "Name required. The database is not a mind reader." });
    return;
  }

  response.status(201).json({ items: createItem(database, name) });
});

app.listen(port, () => {
  console.log(`API listening on http://localhost:${port}`);
});
"#,
        ),
        scaffold_file(
            "server/db.test.js",
            r#"import assert from "node:assert/strict";
import test from "node:test";
import { createItem, listItems, openDatabase } from "./db.js";

test("creates and lists items", () => {
  const database = openDatabase(":memory:");

  assert.deepEqual(listItems(database), []);
  assert.deepEqual(createItem(database, "first slice"), [{ id: 1, name: "first slice" }]);
});
"#,
        ),
        scaffold_file(
            "scripts/dev.mjs",
            r#"import { spawn } from "node:child_process";

const children = [
  spawn("node", ["server/index.js"], { stdio: "inherit" }),
  spawn("npx", ["vite", "--host", "0.0.0.0"], { stdio: "inherit" }),
];

function stop() {
  for (const child of children) {
    child.kill("SIGTERM");
  }
}

process.on("SIGINT", stop);
process.on("SIGTERM", stop);

for (const child of children) {
  child.on("exit", (code) => {
    if (code && code !== 0) {
      stop();
      process.exit(code);
    }
  });
}
"#,
        ),
        scaffold_file(
            "vite.config.js",
            r#"import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": "http://localhost:3001",
    },
  },
});
"#,
        ),
        scaffold_file(
            "README.md",
            &format!(
                r#"# {}

Generated by the Mutagen harness.

```bash
npm install
npm run dev
```

The Vite preview runs at http://localhost:5173 and proxies API requests to Express on http://localhost:3001.
"#,
                capsule.name
            ),
        ),
        scaffold_file("data/.gitkeep", ""),
    ]
}

fn nextjs_postgres_files(capsule: &ProjectCapsule) -> Vec<ScaffoldFile> {
    vec![
        scaffold_file(
            "package.json",
            &format!(
                r#"{{
  "name": "{}",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {{
    "dev": "next dev",
    "test": "node --test",
    "build": "next build",
    "start": "next start"
  }},
  "dependencies": {{
    "next": "^14.2.3",
    "pg": "^8.11.5",
    "react": "^18.2.0",
    "react-dom": "^18.2.0"
  }},
  "devDependencies": {{}}
}}
"#,
                package_name(&capsule.name)
            ),
        ),
        scaffold_file(
            "app/layout.jsx",
            &format!(
                r#"import "./globals.css";

export const metadata = {{
  title: "{}",
  description: "Generated by the Mutagen harness",
}};

export default function RootLayout({{ children }}) {{
  return (
    <html lang="en">
      <body>{{children}}</body>
    </html>
  );
}}
"#,
                capsule.name
            ),
        ),
        scaffold_file(
            "app/page.jsx",
            &format!(
                r#"import {{ listFallbackItems, postgresConnectionString }} from "../lib/items.js";

export default function HomePage() {{
  const items = listFallbackItems();
  const databaseReady = Boolean(postgresConnectionString());

  return (
    <main className="shell">
      <section className="workspace">
        <p className="eyebrow">Next.js + Postgres</p>
        <h1>{}</h1>
        <p className="lede">
          The scaffold is ready for app-router pages, route handlers, and a Postgres-backed service layer.
        </p>
        <div className="status">
          <span>Database</span>
          <strong>{{databaseReady ? "configured" : "waiting for DATABASE_URL"}}</strong>
        </div>
        <ul className="items">
          {{items.map((item) => (
            <li key={{item.id}}>{{item.name}}</li>
          ))}}
        </ul>
      </section>
    </main>
  );
}}
"#,
                capsule.name
            ),
        ),
        scaffold_file(
            "app/api/items/route.js",
            r#"import { NextResponse } from "next/server";
import { createFallbackItem, listFallbackItems } from "../../../lib/items.js";

export async function GET() {
  return NextResponse.json({ items: listFallbackItems() });
}

export async function POST(request) {
  const body = await request.json();
  const item = createFallbackItem(body?.name);

  if (!item) {
    return NextResponse.json(
      { error: "Name required. The database is not a séance." },
      { status: 400 },
    );
  }

  return NextResponse.json({ items: listFallbackItems([item]) }, { status: 201 });
}
"#,
        ),
        scaffold_file(
            "app/globals.css",
            r#":root {
  color: #15191f;
  background: #f5f7fb;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

body {
  margin: 0;
}

.shell {
  min-height: 100vh;
  display: grid;
  place-items: center;
  padding: 32px;
}

.workspace {
  width: min(840px, 100%);
}

.eyebrow {
  color: #4f6578;
  font-size: 0.78rem;
  font-weight: 800;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

h1 {
  margin: 0 0 18px;
  font-size: clamp(2.4rem, 7vw, 5.2rem);
  line-height: 0.95;
}

.lede {
  max-width: 640px;
  color: #526273;
  font-size: 1.05rem;
}

.status,
.items li {
  border: 1px solid #d8e0e8;
  border-radius: 8px;
  background: white;
  padding: 14px 16px;
}

.status {
  display: flex;
  justify-content: space-between;
  gap: 18px;
  margin: 24px 0;
}

.items {
  display: grid;
  gap: 10px;
  padding: 0;
  list-style: none;
}
"#,
        ),
        scaffold_file(
            "lib/items.js",
            r#"const fallbackItems = [
  { id: 1, name: "Wire the first product workflow" },
  { id: 2, name: "Replace fallback storage with Postgres" },
];

export function postgresConnectionString() {
  return process.env.DATABASE_URL || "";
}

export function normalizeItemName(name) {
  return String(name || "").trim();
}

export function listFallbackItems(items = fallbackItems) {
  return [...items];
}

export function createFallbackItem(name) {
  const normalized = normalizeItemName(name);

  if (!normalized) {
    return null;
  }

  return {
    id: Date.now(),
    name: normalized,
  };
}
"#,
        ),
        scaffold_file(
            "tests/items.test.js",
            r#"import assert from "node:assert/strict";
import test from "node:test";
import { createFallbackItem, normalizeItemName } from "../lib/items.js";

test("normalizes item names", () => {
  assert.equal(normalizeItemName("  first useful slice  "), "first useful slice");
});

test("refuses empty item names", () => {
  assert.equal(createFallbackItem("   "), null);
});
"#,
        ),
        scaffold_file(
            "README.md",
            &format!(
                r#"# {}

Generated by the Mutagen harness.

```bash
npm install
npm run dev
```

Set `DATABASE_URL` before replacing the fallback item service with Postgres queries.
"#,
                capsule.name
            ),
        ),
        scaffold_file(
            ".env.example",
            "DATABASE_URL=postgres://user:pass@localhost:5432/app\n",
        ),
    ]
}

fn fastapi_react_files(capsule: &ProjectCapsule) -> Vec<ScaffoldFile> {
    vec![
        scaffold_file(
            "package.json",
            &format!(
                r#"{{
  "name": "{}",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {{
    "dev": "node scripts/dev.mjs",
    "test": "node --test tests/frontend.test.js",
    "build": "vite build"
  }},
  "dependencies": {{
    "@vitejs/plugin-react": "^4.2.1",
    "vite": "^5.1.4",
    "react": "^18.2.0",
    "react-dom": "^18.2.0"
  }},
  "devDependencies": {{}}
}}
"#,
                package_name(&capsule.name)
            ),
        ),
        scaffold_file(
            "requirements.txt",
            "fastapi>=0.111,<1.0\nuvicorn[standard]>=0.29,<1.0\npytest>=8.0,<9.0\nhttpx>=0.27,<1.0\n",
        ),
        scaffold_file("api/__init__.py", ""),
        scaffold_file(
            "api/main.py",
            r#"from fastapi import FastAPI
from pydantic import BaseModel


class ItemCreate(BaseModel):
    name: str


app = FastAPI(title="Mutagen generated API")
items = [{"id": 1, "name": "Connect the first workflow"}]


@app.get("/api/health")
def health():
    return {"ok": True, "status": "ready"}


@app.get("/api/items")
def list_items():
    return {"items": items}


@app.post("/api/items", status_code=201)
def create_item(payload: ItemCreate):
    name = payload.name.strip()

    if not name:
        return {"items": items, "warning": "Name required. Telepathy remains unsupported."}

    item = {"id": len(items) + 1, "name": name}
    items.insert(0, item)
    return {"items": items}
"#,
        ),
        scaffold_file(
            "tests/test_api.py",
            r#"from fastapi.testclient import TestClient

from api.main import app


def test_health_reports_ready():
    client = TestClient(app)

    response = client.get("/api/health")

    assert response.status_code == 200
    assert response.json()["status"] == "ready"
"#,
        ),
        scaffold_file(
            "index.html",
            r#"<div id="root"></div>
<script type="module" src="/src/main.jsx"></script>
"#,
        ),
        scaffold_file(
            "src/main.jsx",
            r#"import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.jsx";
import "./styles.css";

createRoot(document.getElementById("root")).render(<App />);
"#,
        ),
        scaffold_file(
            "src/App.jsx",
            r#"import { useEffect, useState } from "react";

export function App() {
  const [items, setItems] = useState([]);

  useEffect(() => {
    fetch("/api/items")
      .then((response) => response.json())
      .then((payload) => setItems(payload.items));
  }, []);

  return (
    <main className="shell">
      <section className="workspace">
        <p className="eyebrow">FastAPI + React</p>
        <h1>API first, interface close behind.</h1>
        <ul className="items">
          {items.map((item) => (
            <li key={item.id}>{item.name}</li>
          ))}
        </ul>
      </section>
    </main>
  );
}
"#,
        ),
        scaffold_file(
            "src/styles.css",
            r#":root {
  color: #18211f;
  background: #eff6f3;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

body {
  margin: 0;
}

.shell {
  min-height: 100vh;
  display: grid;
  place-items: center;
  padding: 32px;
}

.workspace {
  width: min(760px, 100%);
}

.eyebrow {
  color: #477064;
  font-size: 0.78rem;
  font-weight: 800;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

h1 {
  margin: 0 0 24px;
  font-size: clamp(2.3rem, 7vw, 5rem);
  line-height: 0.96;
}

.items {
  display: grid;
  gap: 10px;
  padding: 0;
  list-style: none;
}

.items li {
  border: 1px solid #cde0d9;
  border-radius: 8px;
  background: white;
  padding: 13px 15px;
}
"#,
        ),
        scaffold_file(
            "tests/frontend.test.js",
            r#"import assert from "node:assert/strict";
import test from "node:test";

test("frontend test harness is wired", () => {
  assert.equal("fastapi-react".includes("react"), true);
});
"#,
        ),
        scaffold_file(
            "scripts/dev.mjs",
            r#"import { spawn } from "node:child_process";

const children = [
  spawn("python", ["-m", "uvicorn", "api.main:app", "--host", "0.0.0.0", "--port", "8000"], { stdio: "inherit" }),
  spawn("npx", ["vite", "--host", "0.0.0.0"], { stdio: "inherit" }),
];

function stop() {
  for (const child of children) {
    child.kill("SIGTERM");
  }
}

process.on("SIGINT", stop);
process.on("SIGTERM", stop);

for (const child of children) {
  child.on("exit", (code) => {
    if (code && code !== 0) {
      stop();
      process.exit(code);
    }
  });
}
"#,
        ),
        scaffold_file(
            "vite.config.js",
            r#"import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": "http://localhost:8000",
    },
  },
});
"#,
        ),
        scaffold_file(
            "README.md",
            &format!(
                r#"# {}

Generated by the Mutagen harness.

```bash
python -m pip install -r requirements.txt
npm install
npm run dev
```

FastAPI runs on http://localhost:8000 and Vite runs on http://localhost:5173.
"#,
                capsule.name
            ),
        ),
    ]
}

fn aspnet_blazor_files(capsule: &ProjectCapsule) -> Vec<ScaffoldFile> {
    vec![
        scaffold_file(
            "MutagenGeneratedApp.csproj",
            r#"<Project Sdk="Microsoft.NET.Sdk.Web">
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
  </PropertyGroup>
</Project>
"#,
        ),
        scaffold_file(
            "Program.cs",
            r#"using MutagenGeneratedApp.Components;

var builder = WebApplication.CreateBuilder(args);

builder.Services
    .AddRazorComponents()
    .AddInteractiveServerComponents();

var app = builder.Build();

app.UseStaticFiles();
app.UseAntiforgery();
app.MapRazorComponents<App>()
    .AddInteractiveServerRenderMode();

app.Run();
"#,
        ),
        scaffold_file(
            "Components/_Imports.razor",
            r#"@using Microsoft.AspNetCore.Components.Routing
@using Microsoft.AspNetCore.Components.Web
@using MutagenGeneratedApp.Components
@using MutagenGeneratedApp.Components.Layout
"#,
        ),
        scaffold_file(
            "Components/App.razor",
            &format!(
                r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>{}</title>
    <base href="/" />
    <link rel="stylesheet" href="app.css" />
    <HeadOutlet />
</head>
<body>
    <Routes />
    <script src="_framework/blazor.web.js"></script>
</body>
</html>
"#,
                capsule.name
            ),
        ),
        scaffold_file(
            "Components/Routes.razor",
            r#"<Router AppAssembly="typeof(Program).Assembly">
    <Found Context="routeData">
        <RouteView RouteData="routeData" DefaultLayout="typeof(MainLayout)" />
        <FocusOnNavigate RouteData="routeData" Selector="h1" />
    </Found>
    <NotFound>
        <LayoutView Layout="typeof(MainLayout)">
            <main class="shell">
                <section class="workspace">
                    <p class="eyebrow">Not found</p>
                    <h1>Nothing lives here yet.</h1>
                </section>
            </main>
        </LayoutView>
    </NotFound>
</Router>
"#,
        ),
        scaffold_file(
            "Components/Layout/MainLayout.razor",
            r#"@inherits LayoutComponentBase

@Body
"#,
        ),
        scaffold_file(
            "Components/Pages/Home.razor",
            &format!(
                r#"@page "/"

<PageTitle>{}</PageTitle>

<main class="shell">
    <section class="workspace">
        <p class="eyebrow">ASP.NET Core + Blazor</p>
        <h1>{}</h1>
        <p class="lede">A server-rendered Blazor surface is ready for components, services, and real product behavior.</p>
        <ul class="items">
            <li>Define the first workflow</li>
            <li>Add application services</li>
            <li>Wire persistence when the domain stops changing its mind</li>
        </ul>
    </section>
</main>
"#,
                capsule.name, capsule.name
            ),
        ),
        scaffold_file(
            "wwwroot/app.css",
            r#":root {
  color: #171b24;
  background: #f3f5f8;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

body {
  margin: 0;
}

.shell {
  min-height: 100vh;
  display: grid;
  place-items: center;
  padding: 32px;
}

.workspace {
  width: min(820px, 100%);
}

.eyebrow {
  color: #59677a;
  font-size: 0.78rem;
  font-weight: 800;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

h1 {
  margin: 0 0 18px;
  font-size: clamp(2.3rem, 7vw, 5rem);
  line-height: 0.96;
}

.lede {
  color: #526073;
  max-width: 660px;
}

.items {
  display: grid;
  gap: 10px;
  padding: 0;
  list-style: none;
}

.items li {
  border: 1px solid #d7dde6;
  border-radius: 8px;
  background: white;
  padding: 13px 15px;
}
"#,
        ),
        scaffold_file(
            "README.md",
            &format!(
                r#"# {}

Generated by the Mutagen harness.

```bash
dotnet restore
dotnet watch run
```

The Blazor preview uses the configured ASP.NET Core launch profile or http://localhost:5000.
"#,
                capsule.name
            ),
        ),
    ]
}

fn cloudflare_worker_files(capsule: &ProjectCapsule) -> Vec<ScaffoldFile> {
    vec![
        scaffold_file(
            "package.json",
            &format!(
                r#"{{
  "name": "{}",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {{
    "dev": "wrangler dev src/index.js --local --port 8787",
    "test": "node --test",
    "build": "node scripts/build.mjs",
    "deploy": "wrangler deploy"
  }},
  "dependencies": {{}},
  "devDependencies": {{
    "wrangler": "^3.57.2"
  }}
}}
"#,
                package_name(&capsule.name)
            ),
        ),
        scaffold_file(
            "src/index.js",
            r#"export function createResponse(pathname) {
  return {
    ok: true,
    message: "Cloudflare Worker scaffold is ready",
    pathname,
  };
}

export default {
  async fetch(request) {
    const url = new URL(request.url);

    if (url.pathname === "/api/health") {
      return Response.json(createResponse(url.pathname));
    }

    return new Response("Mutagen Worker scaffold", {
      headers: { "content-type": "text/plain; charset=utf-8" },
    });
  },
};
"#,
        ),
        scaffold_file(
            "test/index.test.js",
            r#"import assert from "node:assert/strict";
import test from "node:test";
import { createResponse } from "../src/index.js";

test("creates health payloads", () => {
  assert.deepEqual(createResponse("/api/health"), {
    ok: true,
    message: "Cloudflare Worker scaffold is ready",
    pathname: "/api/health",
  });
});
"#,
        ),
        scaffold_file(
            "scripts/build.mjs",
            r#"const worker = await import("../src/index.js");

if (typeof worker.default?.fetch !== "function") {
  throw new Error("Worker default export must expose fetch. Tiny contract, huge consequences.");
}

console.log("worker build check passed");
"#,
        ),
        scaffold_file(
            "wrangler.toml",
            &format!(
                r#"name = "{}"
main = "src/index.js"
compatibility_date = "2024-06-01"
"#,
                package_name(&capsule.name)
            ),
        ),
        scaffold_file(
            "README.md",
            &format!(
                r#"# {}

Generated by the Mutagen harness.

```bash
npm install
npm run dev
```

The local Worker preview runs at http://localhost:8787.
"#,
                capsule.name
            ),
        ),
    ]
}

fn scaffold_file(relative_path: &str, body: &str) -> ScaffoldFile {
    ScaffoldFile {
        relative_path: relative_path.to_string(),
        body: body.to_string(),
    }
}

fn rust_bevy_files(capsule: &ProjectCapsule) -> Vec<ScaffoldFile> {
    vec![
        scaffold_file(
            "Cargo.toml",
            &format!(
                r#"[package]
name = "{}"
version = "0.1.0"
edition = "2024"

[dependencies]
bevy = "0.18.1"
"#,
                package_name(&capsule.name)
            ),
        ),
        scaffold_file(
            "src/main.rs",
            &format!(
                r#"use bevy::prelude::*;

fn main() {{
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {{
            primary_window: Some(Window {{
                title: "{}".to_string(),
                resolution: (960, 540).into(),
                ..default()
            }}),
            ..default()
        }}))
        .add_systems(Startup, setup)
        .run();
}}

fn setup(mut commands: Commands) {{
    commands.spawn(Camera2d);
}}
"#,
                capsule.name
            ),
        ),
        scaffold_file(
            "README.md",
            &format!(
                r#"# {}

Generated by the Mutagen harness.

```bash
cargo fetch
cargo run
```

This scaffold opens a native Bevy window. The harness preview target is `native://bevy`.
"#,
                capsule.name
            ),
        ),
        scaffold_file(
            ".gitignore",
            "/target\n.mutagen/state/\n.mutagen/worktrees/\n",
        ),
    ]
}

fn package_name(name: &str) -> String {
    let mut package = String::new();
    let mut previous_was_dash = false;

    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            package.push(character.to_ascii_lowercase());
            previous_was_dash = false;
        } else if !previous_was_dash {
            package.push('-');
            previous_was_dash = true;
        }
    }

    let package = package.trim_matches('-').to_string();
    if package.is_empty() {
        "mutagen-project".to_string()
    } else {
        package
    }
}

fn blueprint_catalog() -> Vec<StackBlueprint> {
    vec![
        StackBlueprint {
            stack: "nextjs-postgres".to_string(),
            label: "Next.js + Postgres".to_string(),
            description: "React application with a Postgres-backed service layer.".to_string(),
            commands: ProjectCommands {
                setup: "npm install".to_string(),
                dev: "npm run dev".to_string(),
                test: "npm test".to_string(),
                build: "npm run build".to_string(),
            },
            preview: web_preview("http://localhost:3000"),
        },
        StackBlueprint {
            stack: "vite-express-sqlite".to_string(),
            label: "Vite + Express + SQLite".to_string(),
            description:
                "Single-repo web app with a Vite frontend, Express API, and SQLite persistence."
                    .to_string(),
            commands: ProjectCommands {
                setup: "npm install".to_string(),
                dev: "npm run dev".to_string(),
                test: "npm test".to_string(),
                build: "npm run build".to_string(),
            },
            preview: web_preview("http://localhost:5173"),
        },
        StackBlueprint {
            stack: "fastapi-react".to_string(),
            label: "FastAPI + React".to_string(),
            description: "Python API paired with a React frontend.".to_string(),
            commands: ProjectCommands {
                setup: "python -m pip install -r requirements.txt && npm install".to_string(),
                dev: "npm run dev".to_string(),
                test: "python -m pytest && npm test".to_string(),
                build: "npm run build".to_string(),
            },
            preview: web_preview("http://localhost:5173"),
        },
        StackBlueprint {
            stack: "aspnet-blazor".to_string(),
            label: "ASP.NET Core + Blazor".to_string(),
            description: "Full-stack .NET web application with Blazor UI.".to_string(),
            commands: ProjectCommands {
                setup: "dotnet restore".to_string(),
                dev: "dotnet watch run".to_string(),
                test: "dotnet test".to_string(),
                build: "dotnet build".to_string(),
            },
            preview: web_preview("http://localhost:5000"),
        },
        StackBlueprint {
            stack: "cloudflare-worker".to_string(),
            label: "Cloudflare Worker".to_string(),
            description: "Edge-first application targeting Cloudflare Workers.".to_string(),
            commands: ProjectCommands {
                setup: "npm install".to_string(),
                dev: "npm run dev".to_string(),
                test: "npm test".to_string(),
                build: "npm run build".to_string(),
            },
            preview: web_preview("http://localhost:8787"),
        },
        StackBlueprint {
            stack: "rust-bevy".to_string(),
            label: "Rust + Bevy".to_string(),
            description: "Rust game or interactive simulation using the Bevy engine.".to_string(),
            commands: ProjectCommands {
                setup: "cargo fetch".to_string(),
                dev: "cargo run".to_string(),
                test: "cargo test".to_string(),
                build: "cargo build --release".to_string(),
            },
            preview: ProjectPreview {
                url: "native://bevy".to_string(),
                command_kind: ProjectCommandKind::Dev,
                readiness_timeout_seconds: 120,
            },
        },
    ]
}

fn web_preview(url: &str) -> ProjectPreview {
    ProjectPreview {
        url: url.to_string(),
        command_kind: ProjectCommandKind::Dev,
        readiness_timeout_seconds: 60,
    }
}

fn blueprint_for(stack: &str) -> Result<StackBlueprint> {
    blueprint_catalog()
        .into_iter()
        .find(|blueprint| blueprint.stack == stack)
        .ok_or_else(|| {
            let supported = blueprint_catalog()
                .into_iter()
                .map(|blueprint| blueprint.stack)
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::anyhow!("unsupported stack `{stack}`; supported stacks: {supported}")
        })
}

fn load_capsule(path: &Path) -> Result<ProjectCapsule> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read project capsule at {}", display_path(path)))?;

    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse project capsule at {}", display_path(path)))
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    Ok(std::env::current_dir()
        .context("failed to resolve current directory")?
        .join(path))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(path)
            )
        })?;
    }

    let body = serde_json::to_string_pretty(value).context("failed to serialize project JSON")?;
    fs::write(path, format!("{body}\n"))
        .with_context(|| format!("failed to write {}", display_path(path)))
}

fn write_json_if_missing(
    workspace_root: &Path,
    relative_path: &str,
    value: &serde_json::Value,
    force: bool,
    created_paths: &mut Vec<String>,
) -> Result<()> {
    let body = serde_json::to_string_pretty(value).context("failed to serialize scaffold JSON")?;
    write_if_missing(
        workspace_root,
        relative_path,
        &format!("{body}\n"),
        force,
        created_paths,
    )
}

fn write_if_missing(
    workspace_root: &Path,
    relative_path: &str,
    body: &str,
    force: bool,
    created_paths: &mut Vec<String>,
) -> Result<()> {
    let path = workspace_root.join(relative_path);
    if path.exists() && !force {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(&path)
            )
        })?;
    }

    fs::write(&path, body).with_context(|| format!("failed to write {}", display_path(&path)))?;
    created_paths.push(relative_display(relative_path));
    Ok(())
}

fn create_dir_if_missing(
    workspace_root: &Path,
    relative_path: &str,
    created_paths: &mut Vec<String>,
) -> Result<()> {
    let path = workspace_root.join(relative_path);
    if path.exists() {
        return Ok(());
    }

    fs::create_dir_all(&path)
        .with_context(|| format!("failed to create directory {}", display_path(&path)))?;
    created_paths.push(relative_display(relative_path));
    Ok(())
}

fn relative_display(path: impl AsRef<str>) -> String {
    path.as_ref().replace('\\', "/")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn markdown_section_body(document: &str, heading: &str) -> Option<String> {
    let section_header = format!("## {heading}");
    let start = document.find(&section_header)?;
    let after_header = document[start..]
        .find('\n')
        .map(|offset| start + offset + 1)
        .unwrap_or(document.len());
    let next_section = document[after_header..]
        .find("\n## ")
        .map(|offset| after_header + offset + 1)
        .unwrap_or(document.len());
    Some(document[after_header..next_section].trim().to_string())
}

fn upsert_markdown_section(document: &str, heading: &str, body: &str) -> String {
    let section_header = format!("## {heading}");
    let section_body = body.trim();

    if let Some(start) = document.find(&section_header) {
        let after_header = document[start..]
            .find('\n')
            .map(|offset| start + offset + 1)
            .unwrap_or(document.len());
        let next_section = document[after_header..]
            .find("\n## ")
            .map(|offset| after_header + offset + 1)
            .unwrap_or(document.len());
        let mut updated = String::new();
        updated.push_str(document[..start].trim_end());
        updated.push_str(&format!("\n\n{section_header}\n\n{section_body}\n"));
        if next_section < document.len() {
            updated.push('\n');
            updated.push_str(document[next_section..].trim_start());
        }
        return updated.trim().to_string() + "\n";
    }

    let prefix = document.trim_end();
    if prefix.is_empty() {
        format!("{section_header}\n\n{section_body}\n")
    } else {
        format!("{prefix}\n\n{section_header}\n\n{section_body}\n")
    }
}

fn append_markdown_section_entry(document: &str, heading: &str, entry: &str) -> String {
    let section_header = format!("## {heading}");
    let entry_body = entry.trim();

    if let Some(start) = document.find(&section_header) {
        let after_header = document[start..]
            .find('\n')
            .map(|offset| start + offset + 1)
            .unwrap_or(document.len());
        let next_section = document[after_header..]
            .find("\n## ")
            .map(|offset| after_header + offset + 1)
            .unwrap_or(document.len());
        let mut updated = String::new();
        updated.push_str(document[..next_section].trim_end());
        updated.push_str(&format!("\n\n{entry_body}\n"));
        if next_section < document.len() {
            updated.push('\n');
            updated.push_str(document[next_section..].trim_start());
        }
        return updated.trim().to_string() + "\n";
    }

    let prefix = document.trim_end();
    if prefix.is_empty() {
        format!("{section_header}\n\n{entry_body}\n")
    } else {
        format!("{prefix}\n\n{section_header}\n\n{entry_body}\n")
    }
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_text(value: &str, limit: usize) -> String {
    let normalized = normalize_text(value);
    if normalized.chars().count() <= limit {
        return normalized;
    }

    let truncated = normalized.chars().take(limit).collect::<String>();
    let trimmed = truncated.trim_end();
    format!("{trimmed}...")
}

fn feature_id(title: &str) -> String {
    let nanos = OffsetDateTime::now_utc().unix_timestamp_nanos();
    let slug = package_name(title);
    format!("feature-{nanos}-{slug}")
}
