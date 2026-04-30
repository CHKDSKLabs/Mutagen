use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use mutagen_harness::adapter::{HostKind, adapter_for, resolved_host_profile};
use mutagen_harness::amend_scope::{AmendScopeOptions, MutationKind, amend_scope};
use mutagen_harness::cohort::{PrepareCohortOptions, prepare_cohort};
use mutagen_harness::cohort_apply::{ApplyCohortDispatchOptions, apply_cohort_dispatch};
use mutagen_harness::cohort_dispatch::{DispatchCohortMembersOptions, dispatch_cohort_members};
use mutagen_harness::cohort_reconcile::{ReconcileCohortMemberOptions, reconcile_cohort_member};
use mutagen_harness::cohort_result::{
    CollectCohortMemberResultOptions, collect_cohort_member_result,
};
use mutagen_harness::cohort_worktree::{
    CleanupCohortWorktreesOptions, MaterializeCohortWorktreesOptions, cleanup_cohort_worktrees,
    materialize_cohort_worktrees,
};
use mutagen_harness::config::load_workflow_config_file;
use mutagen_harness::dispatch::{AuthorDispatchKind, PrepareDispatchOptions, prepare_dispatch};
use mutagen_harness::finalize::{FinalizeSliceOptions, finalize_slice};
use mutagen_harness::inference::{ChatCompletionOptions, ChatMessage, complete_chat};
use mutagen_harness::model_registry::{
    InferenceProvider, find_model, list_models, list_models_for, resolve_provider_model_id,
};
use mutagen_harness::project::{
    ProjectAddFeatureOptions, ProjectApplyBlueprintOptions, ProjectCommandKind,
    ProjectCreateOptions, ProjectDashboardOptions, ProjectDoctorOptions,
    ProjectEnqueueFeatureOptions, ProjectExecuteFeatureOptions, ProjectFeatureFlowOptions,
    ProjectFeatureProgressOptions, ProjectFeatureStatusOptions, ProjectFeaturesOptions,
    ProjectInitOptions, ProjectInspectOptions, ProjectIntakeOptions, ProjectPlanFeatureOptions,
    ProjectPreviewCheckOptions, ProjectPreviewLifecycleOptions, ProjectPreviewPlanOptions,
    ProjectRepairOptions, ProjectRunCommandOptions, ProjectScaffoldOptions,
    ProjectSliceFeatureOptions, ProjectStatusOptions, ProjectVerifyGeneratedOptions, add_feature,
    apply_blueprint, create_project, dashboard_project, doctor_project, enqueue_feature,
    execute_feature, feature_flow, feature_progress, feature_status, init_project, inspect_project,
    list_blueprints, list_features, plan_feature, preview_check, preview_plan, preview_start,
    preview_status, preview_stop, project_intake, repair_project, run_project_command,
    scaffold_project, slice_feature, status_project, verify_generated_project,
};
use mutagen_harness::queue::{
    BishopVerdict, KaraiStructuralVerdict, SliceStatus, TigerClawVerdict,
};
use mutagen_harness::queue_update::{UpdateSliceOptions, update_slice};
use mutagen_harness::review::{ReviewDecisionOptions, review_decision};
use mutagen_harness::review_record::{RecordReviewVerdictOptions, record_review_verdict};
use mutagen_harness::runner::{
    DispatchStageOptions, QUEUE_CONTRACT_HASH_BASIS, RunCohortOnceOptions, RunExecuteNextOptions,
    RunSliceOnceOptions, RunnerOutcome, dispatch_stage, queue_contract_hash, run_cohort_once,
    run_execute_next, run_slice_once,
};
use mutagen_harness::runtime::{PrepareNextOptions, prepare_next};
use mutagen_harness::scope_violation::{ScopeViolationOptions, scope_violation};
use mutagen_harness::selected_slice::{PrepareSelectedSliceOptions, prepare_selected_slice};
use mutagen_harness::state::Stage;
use mutagen_harness::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use mutagen_harness::state_update::{ApplyStateUpdateOptions, apply_state_update_for_slice};
use mutagen_harness::structural::{StructuralCheckOptions, structural_check};
use mutagen_harness::validation::validate_queue_file;
use std::path::PathBuf;
use std::time::Duration;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum InferenceProviderArg {
    Ollama,
    Lmstudio,
}

impl InferenceProviderArg {
    fn into_provider(self) -> InferenceProvider {
        match self {
            Self::Ollama => InferenceProvider::Ollama,
            Self::Lmstudio => InferenceProvider::LmStudio,
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "mutagen-harness")]
#[command(about = "Canonical runtime scaffold for the mutagen harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    PrepareNext {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".claude/workflow.json")]
        workflow_config: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long, value_enum, default_value_t = HostKind::Stub)]
        host: HostKind,
        #[arg(long)]
        dry_run: bool,
    },
    PrepareSelectedSlice {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".claude/workflow.json")]
        workflow_config: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long)]
        slice_id: String,
        #[arg(long, value_enum, default_value_t = HostKind::Stub)]
        host: HostKind,
        #[arg(long)]
        dry_run: bool,
    },
    PrepareCohort {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".claude/workflow.json")]
        workflow_config: PathBuf,
        #[arg(long, value_enum, default_value_t = HostKind::Stub)]
        host: HostKind,
        #[arg(long)]
        dry_run: bool,
    },
    ReconcileCohortMember {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        worktree_root: PathBuf,
        #[arg(long)]
        slice_id: String,
        #[arg(long)]
        run_output: PathBuf,
        #[arg(long = "merged-path-owner")]
        merged_path_owners: Vec<String>,
    },
    DispatchCohortMembers {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        runner_script: PathBuf,
        #[arg(long, value_enum, default_value_t = HostKind::Stub)]
        host: HostKind,
        #[arg(long = "member-json", required = true)]
        member_json: Vec<String>,
    },
    DispatchStage {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long, default_value = ".mutagen/state/author-output")]
        author_output_dir: PathBuf,
        #[arg(long, default_value = ".mutagen/state/dispatch")]
        dispatch_root: PathBuf,
        #[arg(long, default_value = "slices/slicemap.md")]
        slicemap: PathBuf,
        #[arg(long, default_value = "slices/queue.md")]
        legacy: PathBuf,
        #[arg(long, value_enum, default_value_t = HostKind::Codex)]
        host: HostKind,
        #[arg(long)]
        qa_report: Option<PathBuf>,
        #[arg(long)]
        latest_qa_report: Option<PathBuf>,
        #[arg(long)]
        slice_id: String,
        #[arg(long, value_enum)]
        dispatch_kind: Option<AuthorDispatchKind>,
        #[arg(long)]
        mutagen_root: Option<PathBuf>,
    },
    RunSliceOnce {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/queue-validation.json")]
        queue_validation: PathBuf,
        #[arg(long, default_value = ".claude/workflow.json")]
        workflow_config: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long, default_value = ".mutagen/state/author-output")]
        author_output_dir: PathBuf,
        #[arg(long, default_value = ".mutagen/state/dispatch")]
        dispatch_root: PathBuf,
        #[arg(long, default_value = ".mutagen/state/dispatch-log.jsonl")]
        dispatch_log: PathBuf,
        #[arg(long, default_value = "slices")]
        summary_root: PathBuf,
        #[arg(long, default_value = "slices/slicemap.md")]
        slicemap: PathBuf,
        #[arg(long, default_value = "slices/queue.md")]
        legacy: PathBuf,
        #[arg(long, value_enum, default_value_t = HostKind::Codex)]
        host: HostKind,
        #[arg(long)]
        slice_id: Option<String>,
        #[arg(long)]
        mutagen_root: Option<PathBuf>,
    },
    RunCohortOnce {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/queue-validation.json")]
        queue_validation: PathBuf,
        #[arg(long, default_value = ".claude/workflow.json")]
        workflow_config: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long, default_value = ".mutagen/state/author-output")]
        author_output_dir: PathBuf,
        #[arg(long, default_value = ".mutagen/state/dispatch")]
        dispatch_root: PathBuf,
        #[arg(long, default_value = ".mutagen/state/dispatch-log.jsonl")]
        dispatch_log: PathBuf,
        #[arg(long, default_value = "slices")]
        summary_root: PathBuf,
        #[arg(long, default_value = "slices/slicemap.md")]
        slicemap: PathBuf,
        #[arg(long, default_value = "slices/queue.md")]
        legacy: PathBuf,
        #[arg(long, value_enum, default_value_t = HostKind::Codex)]
        host: HostKind,
        #[arg(long)]
        mutagen_root: Option<PathBuf>,
    },
    RunExecuteNext {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/queue-validation.json")]
        queue_validation: PathBuf,
        #[arg(long, default_value = ".claude/workflow.json")]
        workflow_config: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long, default_value = ".mutagen/state/author-output")]
        author_output_dir: PathBuf,
        #[arg(long, default_value = ".mutagen/state/dispatch")]
        dispatch_root: PathBuf,
        #[arg(long, default_value = ".mutagen/state/dispatch-log.jsonl")]
        dispatch_log: PathBuf,
        #[arg(long, default_value = "slices")]
        summary_root: PathBuf,
        #[arg(long, default_value = "slices/slicemap.md")]
        slicemap: PathBuf,
        #[arg(long, default_value = "slices/queue.md")]
        legacy: PathBuf,
        #[arg(long, value_enum, default_value_t = HostKind::Codex)]
        host: HostKind,
        #[arg(long, default_value_t = 1000)]
        max_loops: u32,
        #[arg(long)]
        mutagen_root: Option<PathBuf>,
    },
    ApplyCohortDispatch {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/dispatch-log.jsonl")]
        dispatch_log: PathBuf,
        #[arg(long = "member-json", required = true)]
        member_json: Vec<String>,
    },
    CollectCohortMemberResult {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        worktree_root: PathBuf,
        #[arg(long)]
        slice_id: String,
        #[arg(long)]
        result_path: PathBuf,
        #[arg(long)]
        status_path: PathBuf,
    },
    MaterializeCohortWorktrees {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long = "slice-id", required = true)]
        slice_ids: Vec<String>,
    },
    CleanupCohortWorktrees {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        worktree_root: PathBuf,
    },
    HostCapabilities {
        #[arg(long, value_enum, default_value_t = HostKind::Stub)]
        host: HostKind,
    },
    HostProfile {
        #[arg(long, default_value = ".claude/workflow.json")]
        workflow_config: PathBuf,
        #[arg(long, value_enum, default_value_t = HostKind::Stub)]
        host: HostKind,
    },
    SupportedModels {
        /// Filter the registry to a specific provider (omit for all entries).
        #[arg(long, value_enum)]
        provider: Option<InferenceProviderArg>,
    },
    CompleteChat {
        #[arg(long, value_enum)]
        provider: InferenceProviderArg,
        /// Either a registry key (e.g. `qwen2.5-coder-14b`) or a raw provider model id.
        #[arg(long)]
        model: String,
        /// Override the provider's default endpoint.
        #[arg(long)]
        endpoint: Option<String>,
        /// File path containing the user prompt body.
        #[arg(long, conflicts_with = "prompt")]
        prompt_file: Option<PathBuf>,
        /// Inline user prompt body.
        #[arg(long, conflicts_with = "prompt_file")]
        prompt: Option<String>,
        /// Optional system prompt.
        #[arg(long)]
        system: Option<String>,
        /// Optional system prompt loaded from a file.
        #[arg(long, conflicts_with = "system")]
        system_file: Option<PathBuf>,
        #[arg(long, default_value_t = 0.0)]
        temperature: f32,
        #[arg(long)]
        max_tokens: Option<u32>,
        #[arg(long)]
        top_p: Option<f32>,
        #[arg(long, default_value_t = 120)]
        timeout_secs: u64,
        /// Write the assistant text to this file in addition to stdout.
        #[arg(long)]
        output_file: Option<PathBuf>,
        /// Print only the assistant text on stdout instead of the full JSON envelope.
        #[arg(long)]
        text_only: bool,
    },
    ValidateQueue {
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
    },
    PrepareDispatch {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long, default_value = ".mutagen/state/author-output")]
        author_output_dir: PathBuf,
        #[arg(long, default_value = ".mutagen/state/dispatch")]
        dispatch_root: PathBuf,
        #[arg(long)]
        qa_report: Option<PathBuf>,
        #[arg(long)]
        latest_qa_report: Option<PathBuf>,
        #[arg(long)]
        slice_id: String,
        #[arg(long, value_enum)]
        dispatch_kind: Option<AuthorDispatchKind>,
    },
    StructuralCheck {
        slice_id: String,
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/author-output")]
        author_output_dir: PathBuf,
        #[arg(long, default_value = "plugins/mutagen/scripts/slice_loc.sh")]
        loc_script: PathBuf,
    },
    UpdateSlice {
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long)]
        slice_id: String,
        #[arg(long, value_enum)]
        status: Option<SliceStatus>,
        #[arg(long)]
        attempts: Option<u32>,
        #[arg(long)]
        micro_corrections_used: Option<u32>,
        #[arg(long, value_enum)]
        karai_structural: Option<KaraiStructuralVerdict>,
        #[arg(long, value_enum)]
        bishop: Option<BishopVerdict>,
        #[arg(long, value_enum)]
        tiger_claw: Option<TigerClawVerdict>,
        #[arg(long)]
        micro_correction: Option<bool>,
        #[arg(long)]
        completed_at: Option<String>,
        #[arg(long)]
        clear_completed_at: bool,
        #[arg(long)]
        escalation_reason: Option<String>,
        #[arg(long)]
        clear_escalation_reason: bool,
        /// Set human_check_needed.resolved_at to a specific ISO-8601 timestamp.
        #[arg(long)]
        human_check_resolved_at: Option<String>,
        /// Set human_check_needed.resolved_at to the current UTC time.
        #[arg(long)]
        resolve_human_check: bool,
        /// Clear human_check_needed.resolved_at (re-opens the gate).
        #[arg(long)]
        clear_human_check_resolved_at: bool,
    },
    TransitionActiveSlice {
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long)]
        slice_id: String,
        #[arg(long, value_enum)]
        stage: Stage,
        #[arg(long)]
        active_agent: Option<String>,
        #[arg(long)]
        bump_attempts: bool,
        #[arg(long)]
        bump_micro_corrections: bool,
    },
    FinalizeSlice {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long, default_value = ".mutagen/state/dispatch-log.jsonl")]
        dispatch_log: PathBuf,
        #[arg(long, default_value = "slices")]
        summary_root: PathBuf,
        #[arg(long)]
        slice_id: String,
        #[arg(long)]
        completed_at: String,
    },
    ApplyStateUpdate {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long)]
        slice_id: String,
        #[arg(long)]
        author_output: Option<PathBuf>,
    },
    ReviewDecision {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long)]
        qa_report: Option<PathBuf>,
        #[arg(long)]
        latest_qa_report: Option<PathBuf>,
        #[arg(long)]
        slice_id: String,
    },
    RecordReviewVerdict {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long)]
        qa_report: Option<PathBuf>,
        #[arg(long)]
        latest_qa_report: Option<PathBuf>,
        #[arg(long)]
        slice_id: String,
    },
    ScopeViolation {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long, default_value = ".mutagen/state/scope-violation.json")]
        violation_report: PathBuf,
    },
    AmendScope {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, default_value = "slices/queue.json")]
        queue: PathBuf,
        #[arg(long, default_value = ".mutagen/state/active-slice.json")]
        active_state: PathBuf,
        #[arg(long, default_value = ".mutagen/state/amendments.jsonl")]
        amendments_log: PathBuf,
        #[arg(long = "requested-glob", required = true)]
        requested_globs: Vec<String>,
        #[arg(long, value_enum)]
        mutation_kind: MutationKind,
        #[arg(long)]
        reason: String,
    },
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    Init {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "unspecified")]
        stack: String,
        #[arg(long, default_value = "unspecified")]
        design_system: String,
        #[arg(long)]
        deploy_target: Option<String>,
        #[arg(long)]
        force: bool,
    },
    Create {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        name: String,
        #[arg(long)]
        stack: String,
        #[arg(long, default_value = "unspecified")]
        design_system: String,
        #[arg(long)]
        deploy_target: Option<String>,
        #[arg(long)]
        force: bool,
    },
    Inspect {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
    },
    Doctor {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
    },
    Status {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
    },
    Intake {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        queue_feature: bool,
        #[arg(long)]
        force: bool,
    },
    AddFeature {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        description: String,
    },
    Features {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
    },
    PlanFeature {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        feature_id: String,
        #[arg(long)]
        force: bool,
    },
    FeatureStatus {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        feature_id: String,
    },
    SliceFeature {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        feature_id: String,
        #[arg(long)]
        force: bool,
    },
    EnqueueFeature {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        feature_id: String,
        #[arg(long)]
        force: bool,
    },
    FeatureFlow {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        description: String,
        #[arg(long)]
        force: bool,
    },
    ExecuteFeature {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        feature_id: String,
        #[arg(long, value_enum, default_value_t = HostKind::Stub)]
        host: HostKind,
        #[arg(long)]
        dry_run: bool,
    },
    FeatureProgress {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        feature_id: String,
    },
    Dashboard {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
    },
    Blueprints,
    ApplyBlueprint {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        stack: Option<String>,
    },
    Scaffold {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        force: bool,
    },
    Repair {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        scaffold: bool,
        #[arg(long)]
        force: bool,
    },
    RunCommand {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long, value_enum)]
        kind: ProjectCommandKind,
        #[arg(long)]
        dry_run: bool,
    },
    VerifyGenerated {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
    },
    PreviewPlan {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
    },
    PreviewStart {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
    },
    PreviewStatus {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
    },
    PreviewStop {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
    },
    PreviewCheck {
        #[arg(long, default_value = ".")]
        workspace_root: PathBuf,
    },
}

fn print_runner_outcome(outcome: RunnerOutcome) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&outcome.payload)?);

    if outcome.exit_code != 0 {
        std::process::exit(outcome.exit_code);
    }

    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Project { command } => match command {
            ProjectCommand::Init {
                workspace_root,
                name,
                stack,
                design_system,
                deploy_target,
                force,
            } => {
                let result = init_project(ProjectInitOptions {
                    workspace_root,
                    name,
                    stack,
                    design_system,
                    deploy_target,
                    force,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::Create {
                workspace_root,
                name,
                stack,
                design_system,
                deploy_target,
                force,
            } => {
                let result = create_project(ProjectCreateOptions {
                    workspace_root,
                    name,
                    stack,
                    design_system,
                    deploy_target,
                    force,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::Inspect { workspace_root } => {
                let result = inspect_project(ProjectInspectOptions { workspace_root })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::Doctor { workspace_root } => {
                let result = doctor_project(ProjectDoctorOptions { workspace_root })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::Status { workspace_root } => {
                let result = status_project(ProjectStatusOptions { workspace_root })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::Intake {
                workspace_root,
                prompt,
                queue_feature,
                force,
            } => {
                let result = project_intake(ProjectIntakeOptions {
                    workspace_root,
                    prompt,
                    queue_feature,
                    force,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::AddFeature {
                workspace_root,
                title,
                description,
            } => {
                let result = add_feature(ProjectAddFeatureOptions {
                    workspace_root,
                    title,
                    description,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::Features { workspace_root } => {
                let result = list_features(ProjectFeaturesOptions { workspace_root })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::PlanFeature {
                workspace_root,
                feature_id,
                force,
            } => {
                let result = plan_feature(ProjectPlanFeatureOptions {
                    workspace_root,
                    feature_id,
                    force,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::FeatureStatus {
                workspace_root,
                feature_id,
            } => {
                let result = feature_status(ProjectFeatureStatusOptions {
                    workspace_root,
                    feature_id,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::SliceFeature {
                workspace_root,
                feature_id,
                force,
            } => {
                let result = slice_feature(ProjectSliceFeatureOptions {
                    workspace_root,
                    feature_id,
                    force,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::EnqueueFeature {
                workspace_root,
                feature_id,
                force,
            } => {
                let result = enqueue_feature(ProjectEnqueueFeatureOptions {
                    workspace_root,
                    feature_id,
                    force,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::FeatureFlow {
                workspace_root,
                title,
                description,
                force,
            } => {
                let result = feature_flow(ProjectFeatureFlowOptions {
                    workspace_root,
                    title,
                    description,
                    force,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::ExecuteFeature {
                workspace_root,
                feature_id,
                host,
                dry_run,
            } => {
                let result = execute_feature(ProjectExecuteFeatureOptions {
                    workspace_root,
                    feature_id,
                    host,
                    dry_run,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::FeatureProgress {
                workspace_root,
                feature_id,
            } => {
                let result = feature_progress(ProjectFeatureProgressOptions {
                    workspace_root,
                    feature_id,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::Dashboard { workspace_root } => {
                let result = dashboard_project(ProjectDashboardOptions { workspace_root })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::Blueprints => {
                let result = list_blueprints();

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::ApplyBlueprint {
                workspace_root,
                stack,
            } => {
                let result = apply_blueprint(ProjectApplyBlueprintOptions {
                    workspace_root,
                    stack,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::Scaffold {
                workspace_root,
                force,
            } => {
                let result = scaffold_project(ProjectScaffoldOptions {
                    workspace_root,
                    force,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::Repair {
                workspace_root,
                scaffold,
                force,
            } => {
                let result = repair_project(ProjectRepairOptions {
                    workspace_root,
                    scaffold,
                    force,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::RunCommand {
                workspace_root,
                kind,
                dry_run,
            } => {
                let result = run_project_command(ProjectRunCommandOptions {
                    workspace_root,
                    kind,
                    dry_run,
                })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::VerifyGenerated { workspace_root } => {
                let result =
                    verify_generated_project(ProjectVerifyGeneratedOptions { workspace_root })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::PreviewPlan { workspace_root } => {
                let result = preview_plan(ProjectPreviewPlanOptions { workspace_root })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::PreviewStart { workspace_root } => {
                let result = preview_start(ProjectPreviewLifecycleOptions { workspace_root })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::PreviewStatus { workspace_root } => {
                let result = preview_status(ProjectPreviewLifecycleOptions { workspace_root })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::PreviewStop { workspace_root } => {
                let result = preview_stop(ProjectPreviewLifecycleOptions { workspace_root })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            ProjectCommand::PreviewCheck { workspace_root } => {
                let result = preview_check(ProjectPreviewCheckOptions { workspace_root })?;

                println!("{}", serde_json::to_string_pretty(&result)?);
            }
        },
        Command::PrepareNext {
            workspace_root,
            queue,
            workflow_config,
            active_state,
            host,
            dry_run,
        } => {
            let result = prepare_next(PrepareNextOptions {
                workspace_root,
                queue_path: queue,
                workflow_config_path: workflow_config,
                active_state_path: active_state,
                host,
                dry_run,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::PrepareSelectedSlice {
            workspace_root,
            queue,
            workflow_config,
            active_state,
            slice_id,
            host,
            dry_run,
        } => {
            let result = prepare_selected_slice(PrepareSelectedSliceOptions {
                workspace_root,
                queue_path: queue,
                workflow_config_path: workflow_config,
                active_state_path: active_state,
                slice_id,
                host,
                dry_run,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::PrepareCohort {
            workspace_root,
            queue,
            workflow_config,
            host,
            dry_run,
        } => {
            let result = prepare_cohort(PrepareCohortOptions {
                workspace_root,
                queue_path: queue,
                workflow_config_path: workflow_config,
                host,
                dry_run,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::ReconcileCohortMember {
            workspace_root,
            worktree_root,
            slice_id,
            run_output,
            merged_path_owners,
        } => {
            let result = reconcile_cohort_member(ReconcileCohortMemberOptions {
                workspace_root,
                worktree_root,
                slice_id,
                run_output_path: run_output,
                merged_path_owners,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::DispatchCohortMembers {
            workspace_root,
            runner_script,
            host,
            member_json,
        } => {
            let result = dispatch_cohort_members(DispatchCohortMembersOptions {
                workspace_root,
                runner_script_path: runner_script,
                host,
                member_json,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::DispatchStage {
            workspace_root,
            queue,
            active_state,
            author_output_dir,
            dispatch_root,
            slicemap,
            legacy,
            host,
            qa_report,
            latest_qa_report,
            slice_id,
            dispatch_kind,
            mutagen_root,
        } => {
            print_runner_outcome(dispatch_stage(DispatchStageOptions {
                workspace_root,
                queue_path: queue,
                active_state_path: active_state,
                author_output_dir,
                dispatch_root,
                slicemap_path: slicemap,
                legacy_path: legacy,
                host,
                qa_report_path: qa_report,
                latest_qa_report_path: latest_qa_report,
                slice_id,
                dispatch_kind,
                mutagen_root,
            }))?;
        }
        Command::RunSliceOnce {
            workspace_root,
            queue,
            queue_validation,
            workflow_config,
            active_state,
            author_output_dir,
            dispatch_root,
            dispatch_log,
            summary_root,
            slicemap,
            legacy,
            host,
            slice_id,
            mutagen_root,
        } => {
            print_runner_outcome(run_slice_once(RunSliceOnceOptions {
                workspace_root,
                queue_path: queue,
                queue_validation_path: queue_validation,
                workflow_config_path: workflow_config,
                active_state_path: active_state,
                author_output_dir,
                dispatch_root,
                dispatch_log_path: dispatch_log,
                summary_root,
                slicemap_path: slicemap,
                legacy_path: legacy,
                host,
                slice_id,
                mutagen_root,
            }))?;
        }
        Command::RunCohortOnce {
            workspace_root,
            queue,
            queue_validation,
            workflow_config,
            active_state,
            author_output_dir,
            dispatch_root,
            dispatch_log,
            summary_root,
            slicemap,
            legacy,
            host,
            mutagen_root,
        } => {
            print_runner_outcome(run_cohort_once(RunCohortOnceOptions {
                workspace_root,
                queue_path: queue,
                queue_validation_path: queue_validation,
                workflow_config_path: workflow_config,
                active_state_path: active_state,
                author_output_dir,
                dispatch_root,
                dispatch_log_path: dispatch_log,
                summary_root,
                slicemap_path: slicemap,
                legacy_path: legacy,
                host,
                mutagen_root,
            }))?;
        }
        Command::RunExecuteNext {
            workspace_root,
            queue,
            queue_validation,
            workflow_config,
            active_state,
            author_output_dir,
            dispatch_root,
            dispatch_log,
            summary_root,
            slicemap,
            legacy,
            host,
            max_loops,
            mutagen_root,
        } => {
            print_runner_outcome(run_execute_next(RunExecuteNextOptions {
                workspace_root,
                queue_path: queue,
                queue_validation_path: queue_validation,
                workflow_config_path: workflow_config,
                active_state_path: active_state,
                author_output_dir,
                dispatch_root,
                dispatch_log_path: dispatch_log,
                summary_root,
                slicemap_path: slicemap,
                legacy_path: legacy,
                host,
                max_loops,
                mutagen_root,
            }))?;
        }
        Command::ApplyCohortDispatch {
            workspace_root,
            queue,
            dispatch_log,
            member_json,
        } => {
            let result = apply_cohort_dispatch(ApplyCohortDispatchOptions {
                workspace_root,
                queue_path: queue,
                dispatch_log_path: dispatch_log,
                member_json,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::CollectCohortMemberResult {
            workspace_root,
            worktree_root,
            slice_id,
            result_path,
            status_path,
        } => {
            let result = collect_cohort_member_result(CollectCohortMemberResultOptions {
                workspace_root,
                worktree_root,
                slice_id,
                result_path,
                status_path,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::MaterializeCohortWorktrees {
            workspace_root,
            slice_ids,
        } => {
            let result = materialize_cohort_worktrees(MaterializeCohortWorktreesOptions {
                workspace_root,
                slice_ids,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::CleanupCohortWorktrees {
            workspace_root,
            worktree_root,
        } => {
            let result = cleanup_cohort_worktrees(CleanupCohortWorktreesOptions {
                workspace_root,
                worktree_root,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::HostCapabilities { host } => {
            let adapter = adapter_for(host);
            let capabilities = serde_json::json!({
                "host": host,
                "capabilities": adapter.capabilities(),
                "degraded_features": adapter.capabilities().degraded_features(),
            });

            println!("{}", serde_json::to_string_pretty(&capabilities)?);
        }
        Command::HostProfile {
            workflow_config,
            host,
        } => {
            let workflow_config = load_workflow_config_file(&workflow_config)?;
            let profile = resolved_host_profile(host, &workflow_config);
            println!("{}", serde_json::to_string_pretty(&profile)?);
        }
        Command::SupportedModels { provider } => {
            let envelope = match provider {
                Some(arg) => {
                    let provider = arg.into_provider();
                    serde_json::json!({
                        "provider": provider.name(),
                        "default_endpoint": provider.default_endpoint(),
                        "models": list_models_for(provider),
                    })
                }
                None => serde_json::json!({
                    "providers": [
                        {
                            "name": InferenceProvider::Ollama.name(),
                            "default_endpoint": InferenceProvider::Ollama.default_endpoint(),
                        },
                        {
                            "name": InferenceProvider::LmStudio.name(),
                            "default_endpoint": InferenceProvider::LmStudio.default_endpoint(),
                        }
                    ],
                    "models": list_models(),
                }),
            };
            println!("{}", serde_json::to_string_pretty(&envelope)?);
        }
        Command::CompleteChat {
            provider,
            model,
            endpoint,
            prompt_file,
            prompt,
            system,
            system_file,
            temperature,
            max_tokens,
            top_p,
            timeout_secs,
            output_file,
            text_only,
        } => {
            let provider = provider.into_provider();

            let prompt_text = match (prompt, prompt_file) {
                (Some(text), None) => text,
                (None, Some(path)) => std::fs::read_to_string(&path)
                    .with_context(|| format!("failed to read prompt file {}", path.display()))?,
                (None, None) => bail!("must supply either --prompt or --prompt-file"),
                (Some(_), Some(_)) => unreachable!("clap conflicts_with prevents this"),
            };

            let system_text = match (system, system_file) {
                (Some(text), None) => Some(text),
                (None, Some(path)) => Some(std::fs::read_to_string(&path).with_context(|| {
                    format!("failed to read system prompt file {}", path.display())
                })?),
                (None, None) => None,
                (Some(_), Some(_)) => unreachable!("clap conflicts_with prevents this"),
            };

            let mut messages = Vec::new();
            if let Some(system_text) = system_text {
                messages.push(ChatMessage::system(system_text));
            }
            messages.push(ChatMessage::user(prompt_text));

            // Surface unknown registry keys as a warning rather than an error so
            // callers can pass a raw provider model id (`qwen2.5:14b-q5_K_M`)
            // when they want a quant the registry doesn't enumerate.
            let resolved_id = resolve_provider_model_id(provider, &model)
                .map(|id| id.to_string())
                .unwrap_or_else(|| model.clone());
            if find_model(&model).is_none() && resolved_id == model {
                eprintln!(
                    "note: `{}` is not in the registry — passing it through as a raw {} model id",
                    model,
                    provider.name()
                );
            }

            let endpoint = endpoint.unwrap_or_else(|| provider.default_endpoint().to_string());

            let options = ChatCompletionOptions {
                provider,
                endpoint,
                model_key_or_id: model,
                messages,
                temperature: Some(temperature),
                max_tokens,
                top_p,
                timeout: Duration::from_secs(timeout_secs),
            };

            let response = complete_chat(&options)?;

            if let Some(path) = output_file.as_ref() {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create parent directory for {}", path.display())
                    })?;
                }
                let body = response.first_text().unwrap_or("");
                std::fs::write(path, body)
                    .with_context(|| format!("failed to write output file {}", path.display()))?;
            }

            if text_only {
                println!("{}", response.first_text().unwrap_or(""));
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "provider": provider.name(),
                        "endpoint": options.endpoint,
                        "model_key_or_id": options.model_key_or_id,
                        "resolved_model_id": resolved_id,
                        "id": response.id,
                        "model": response.model,
                        "text": response.first_text(),
                        "finish_reason": response.choices.first().and_then(|c| c.finish_reason.clone()),
                        "usage": response.usage,
                    }))?
                );
            }
        }
        Command::ValidateQueue { queue } => {
            let report = validate_queue_file(&queue)?;
            let mut report = serde_json::to_value(report)?;
            if let Ok(hash) = queue_contract_hash(&queue) {
                report["queue"] = serde_json::json!(queue.to_string_lossy());
                report["queue_contract_hash"] = serde_json::json!(hash);
                report["queue_contract_hash_basis"] = serde_json::json!(QUEUE_CONTRACT_HASH_BASIS);
                report["queue_contract_hash_algorithm"] = serde_json::json!("sha1");
            }
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::PrepareDispatch {
            workspace_root,
            queue,
            active_state,
            author_output_dir,
            dispatch_root,
            qa_report,
            latest_qa_report,
            slice_id,
            dispatch_kind,
        } => {
            let result = prepare_dispatch(PrepareDispatchOptions {
                workspace_root,
                queue_path: queue,
                active_state_path: active_state,
                author_output_dir,
                dispatch_root,
                qa_report_path: qa_report,
                latest_qa_report_path: latest_qa_report,
                slice_id,
                dispatch_kind,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::StructuralCheck {
            slice_id,
            workspace_root,
            queue,
            author_output_dir,
            loc_script,
        } => {
            let report = structural_check(StructuralCheckOptions {
                slice_id,
                workspace_root,
                queue_path: queue,
                author_output_dir,
                loc_script_path: loc_script,
            });

            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::UpdateSlice {
            queue,
            slice_id,
            status,
            attempts,
            micro_corrections_used,
            karai_structural,
            bishop,
            tiger_claw,
            micro_correction,
            completed_at,
            clear_completed_at,
            escalation_reason,
            clear_escalation_reason,
            human_check_resolved_at,
            resolve_human_check,
            clear_human_check_resolved_at,
        } => {
            if resolve_human_check && human_check_resolved_at.is_some() {
                anyhow::bail!(
                    "use either --resolve-human-check or --human-check-resolved-at, not both"
                );
            }
            let resolved_at = if resolve_human_check {
                Some(
                    OffsetDateTime::now_utc()
                        .format(&Rfc3339)
                        .context("failed to format current UTC time")?,
                )
            } else {
                human_check_resolved_at
            };
            let result = update_slice(UpdateSliceOptions {
                queue_path: queue,
                slice_id,
                status,
                attempts,
                micro_corrections_used,
                karai_structural,
                bishop,
                tiger_claw,
                micro_correction,
                completed_at,
                clear_completed_at,
                escalation_reason,
                clear_escalation_reason,
                human_check_resolved_at: resolved_at,
                clear_human_check_resolved_at,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::TransitionActiveSlice {
            queue,
            active_state,
            slice_id,
            stage,
            active_agent,
            bump_attempts,
            bump_micro_corrections,
        } => {
            let result = transition_active_slice(TransitionActiveSliceOptions {
                queue_path: queue,
                active_state_path: active_state,
                slice_id,
                stage,
                active_agent,
                bump_attempts,
                bump_micro_corrections,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::FinalizeSlice {
            workspace_root,
            queue,
            active_state,
            dispatch_log,
            summary_root,
            slice_id,
            completed_at,
        } => {
            let result = finalize_slice(FinalizeSliceOptions {
                workspace_root,
                queue_path: queue,
                active_state_path: active_state,
                dispatch_log_path: dispatch_log,
                summary_root,
                slice_id,
                completed_at,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::ApplyStateUpdate {
            workspace_root,
            queue,
            slice_id,
            author_output,
        } => {
            let result = apply_state_update_for_slice(ApplyStateUpdateOptions {
                workspace_root,
                queue_path: queue,
                slice_id,
                author_output_path: author_output,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::ReviewDecision {
            workspace_root,
            queue,
            active_state,
            qa_report,
            latest_qa_report,
            slice_id,
        } => {
            let result = review_decision(ReviewDecisionOptions {
                workspace_root,
                queue_path: queue,
                active_state_path: active_state,
                qa_report_path: qa_report,
                latest_qa_report_path: latest_qa_report,
                slice_id,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::RecordReviewVerdict {
            workspace_root,
            queue,
            active_state,
            qa_report,
            latest_qa_report,
            slice_id,
        } => {
            let result = record_review_verdict(RecordReviewVerdictOptions {
                workspace_root,
                queue_path: queue,
                active_state_path: active_state,
                qa_report_path: qa_report,
                latest_qa_report_path: latest_qa_report,
                slice_id,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::ScopeViolation {
            workspace_root,
            queue,
            active_state,
            violation_report,
        } => {
            let result = scope_violation(ScopeViolationOptions {
                workspace_root,
                queue_path: queue,
                active_state_path: active_state,
                violation_path: violation_report,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::AmendScope {
            workspace_root,
            queue,
            active_state,
            amendments_log,
            requested_globs,
            mutation_kind,
            reason,
        } => {
            let result = amend_scope(AmendScopeOptions {
                workspace_root,
                queue_path: queue,
                active_state_path: active_state,
                amendments_log_path: amendments_log,
                requested_globs,
                mutation_kind,
                reason,
            })?;

            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }

    Ok(())
}
