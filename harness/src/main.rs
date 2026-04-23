use anyhow::Result;
use clap::{Parser, Subcommand};
use mutagen_harness::adapter::{HostKind, adapter_for, resolved_host_profile};
use mutagen_harness::amend_scope::{AmendScopeOptions, MutationKind, amend_scope};
use mutagen_harness::config::load_workflow_config_file;
use mutagen_harness::dispatch::{AuthorDispatchKind, PrepareDispatchOptions, prepare_dispatch};
use mutagen_harness::finalize::{FinalizeSliceOptions, finalize_slice};
use mutagen_harness::queue::{
    BishopVerdict, KaraiStructuralVerdict, SliceStatus, TigerClawVerdict,
};
use mutagen_harness::queue_update::{UpdateSliceOptions, update_slice};
use mutagen_harness::review::{ReviewDecisionOptions, review_decision};
use mutagen_harness::review_record::{RecordReviewVerdictOptions, record_review_verdict};
use mutagen_harness::runtime::{PrepareNextOptions, prepare_next};
use mutagen_harness::scope_violation::{ScopeViolationOptions, scope_violation};
use mutagen_harness::state::Stage;
use mutagen_harness::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use mutagen_harness::structural::{StructuralCheckOptions, structural_check};
use mutagen_harness::validation::validate_queue_file;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "mutagen-harness")]
#[command(about = "Canonical runtime scaffold for the mutagen harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
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
        Command::ValidateQueue { queue } => {
            let report = validate_queue_file(&queue)?;
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
        } => {
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
