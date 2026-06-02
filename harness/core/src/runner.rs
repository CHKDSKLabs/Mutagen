use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::env;
use std::fmt;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::adapter::HostKind;
use crate::cohort::{PrepareCohortOptions, prepare_cohort};
use crate::cohort_apply::{
    ApplyCohortDispatchOptions, ApplyCohortDispatchResult, apply_cohort_dispatch,
};
use crate::cohort_dispatch::DispatchedCohortMember;
use crate::cohort_result::{CollectCohortMemberResultOptions, collect_cohort_member_result};
use crate::cohort_worktree::{
    CleanupCohortWorktreesOptions, CohortWorktreeMember, MaterializeCohortWorktreesOptions,
    MaterializeCohortWorktreesResult, cleanup_cohort_worktrees, materialize_cohort_worktrees,
};
use crate::dispatch::{AuthorDispatchKind, PrepareDispatchOptions, prepare_dispatch};
use crate::finalize::{FinalizeSliceOptions, finalize_slice};
use crate::queue::{
    BishopVerdict, KaraiStructuralVerdict, SliceQueue, SliceStatus, TigerClawVerdict,
};
use crate::queue_readiness::{QueueReadiness, check_queue_readiness, readiness_failure_value};
use crate::queue_update::{UpdateSliceOptions, update_slice};
use crate::review::{ReviewDecisionOptions, review_decision};
use crate::review_record::{RecordReviewVerdictOptions, record_review_verdict};
use crate::runtime::{PrepareNextOptions, prepare_next};
use crate::selected_slice::{PrepareSelectedSliceOptions, prepare_selected_slice};
use crate::state::{Stage, load_active_slice};
use crate::state_transition::{TransitionActiveSliceOptions, transition_active_slice};
use crate::structural::{StructuralCheckOptions, structural_check};
use crate::validation::load_queue_file;

pub use crate::queue_readiness::{QUEUE_CONTRACT_HASH_BASIS, queue_contract_hash};

#[derive(Debug, Clone)]
pub struct RunnerOutcome {
    pub payload: Value,
    pub exit_code: i32,
}

#[derive(Debug, Clone)]
pub struct DispatchStageOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub active_state_path: PathBuf,
    pub author_output_dir: PathBuf,
    pub dispatch_root: PathBuf,
    pub slicemap_path: PathBuf,
    pub legacy_path: PathBuf,
    pub host: HostKind,
    pub qa_report_path: Option<PathBuf>,
    pub latest_qa_report_path: Option<PathBuf>,
    pub slice_id: String,
    pub dispatch_kind: Option<AuthorDispatchKind>,
    pub mutagen_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct RunSliceOnceOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub queue_validation_path: PathBuf,
    pub workflow_config_path: PathBuf,
    pub active_state_path: PathBuf,
    pub author_output_dir: PathBuf,
    pub dispatch_root: PathBuf,
    pub dispatch_log_path: PathBuf,
    pub summary_root: PathBuf,
    pub slicemap_path: PathBuf,
    pub legacy_path: PathBuf,
    pub host: HostKind,
    pub slice_id: Option<String>,
    pub mutagen_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct RunCohortOnceOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub queue_validation_path: PathBuf,
    pub workflow_config_path: PathBuf,
    pub active_state_path: PathBuf,
    pub author_output_dir: PathBuf,
    pub dispatch_root: PathBuf,
    pub dispatch_log_path: PathBuf,
    pub summary_root: PathBuf,
    pub slicemap_path: PathBuf,
    pub legacy_path: PathBuf,
    pub host: HostKind,
    pub mutagen_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct RunExecuteNextOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub queue_validation_path: PathBuf,
    pub workflow_config_path: PathBuf,
    pub active_state_path: PathBuf,
    pub author_output_dir: PathBuf,
    pub dispatch_root: PathBuf,
    pub dispatch_log_path: PathBuf,
    pub summary_root: PathBuf,
    pub slicemap_path: PathBuf,
    pub legacy_path: PathBuf,
    pub host: HostKind,
    pub max_loops: u32,
    pub mutagen_root: Option<PathBuf>,
}

pub fn dispatch_stage(options: DispatchStageOptions) -> RunnerOutcome {
    match dispatch_stage_value(options) {
        Ok(payload) => RunnerOutcome {
            payload,
            exit_code: 0,
        },
        Err(error) => failure_outcome("dispatch_stage_failed", &format!("{error:#}"), json!({}), 1),
    }
}

pub fn run_slice_once(options: RunSliceOnceOptions) -> RunnerOutcome {
    let mut current_slice_id = String::new();
    match run_slice_once_inner(options, &mut current_slice_id) {
        Ok(outcome) => outcome,
        Err(error) => step_error_outcome(&current_slice_id, error),
    }
}

pub fn run_cohort_once(options: RunCohortOnceOptions) -> RunnerOutcome {
    match run_cohort_once_inner(options) {
        Ok(outcome) => outcome,
        Err(error) => step_error_outcome("", error),
    }
}

pub fn run_execute_next(options: RunExecuteNextOptions) -> RunnerOutcome {
    match run_execute_next_inner(options) {
        Ok(outcome) => outcome,
        Err(error) => step_error_outcome("", error),
    }
}

#[derive(Debug, Clone)]
pub struct ContinueSliceOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub queue_validation_path: PathBuf,
    pub workflow_config_path: PathBuf,
    pub active_state_path: PathBuf,
    pub author_output_dir: PathBuf,
    pub dispatch_root: PathBuf,
    pub dispatch_log_path: PathBuf,
    pub summary_root: PathBuf,
    pub slicemap_path: PathBuf,
    pub legacy_path: PathBuf,
    pub host: HostKind,
    pub slice_id: Option<String>,
    pub mutagen_root: Option<PathBuf>,
}

pub fn run_continue_slice(options: ContinueSliceOptions) -> RunnerOutcome {
    let mut current_slice_id = String::new();
    match run_continue_slice_inner(options, &mut current_slice_id) {
        Ok(outcome) => outcome,
        Err(error) => step_error_outcome(&current_slice_id, error),
    }
}

fn run_slice_once_inner(
    options: RunSliceOnceOptions,
    current_slice_id: &mut String,
) -> Result<RunnerOutcome> {
    let paths = RunnerPaths::resolve(
        options.workspace_root,
        options.queue_path,
        options.queue_validation_path,
        options.workflow_config_path,
        options.active_state_path,
        options.author_output_dir,
        options.dispatch_root,
        options.dispatch_log_path,
        options.summary_root,
        options.slicemap_path,
        options.legacy_path,
    )?;

    ensure_workspace_dirs(&paths.workspace_root)?;
    ensure_no_active_slice(&paths.active_state_path, "slice")?;

    let prepare_output = if let Some(selected_slice_id) = options.slice_id.as_deref() {
        *current_slice_id = selected_slice_id.to_string();
        let result = json_step(
            "prepare_slice",
            prepare_selected_slice(PrepareSelectedSliceOptions {
                workspace_root: paths.workspace_root.clone(),
                queue_path: paths.queue_path.clone(),
                queue_validation_path: paths.queue_validation_path.clone(),
                workflow_config_path: paths.workflow_config_path.clone(),
                active_state_path: paths.active_state_path.clone(),
                slice_id: selected_slice_id.to_string(),
                host: options.host,
                dry_run: false,
            }),
        )?;

        if status_of(&result) == Some("blocked") {
            return Ok(RunnerOutcome {
                payload: result,
                exit_code: 2,
            });
        }

        result
    } else {
        if let QueueReadyResult::NotReady(outcome) =
            queue_ready(&paths.queue_path, &paths.queue_validation_path)?
        {
            return Ok(outcome);
        }

        json_step(
            "prepare_slice",
            prepare_next(PrepareNextOptions {
                workspace_root: paths.workspace_root.clone(),
                queue_path: paths.queue_path.clone(),
                queue_validation_path: paths.queue_validation_path.clone(),
                workflow_config_path: paths.workflow_config_path.clone(),
                active_state_path: paths.active_state_path.clone(),
                host: options.host,
                dry_run: false,
            }),
        )?
    };

    match status_of(&prepare_output).unwrap_or_default() {
        "queue_clear" | "stalled" => {
            return Ok(RunnerOutcome {
                payload: json!({
                    "ok": true,
                    "status": status_of(&prepare_output).unwrap_or_default(),
                    "prepare_next": prepare_output,
                }),
                exit_code: 0,
            });
        }
        "ready" => {
            *current_slice_id = required_string(&prepare_output, "slice_id")?;
        }
        other => {
            bail!(StepFailure::new(
                "prepare_slice",
                format!("slice preparation returned unsupported status '{other}'"),
            ));
        }
    }

    let active_state = load_active_slice(&paths.active_state_path)?;
    let mut max_loops = active_state.max_retries + active_state.max_micro_corrections + 4;
    if max_loops == 0 {
        max_loops = 7;
    }

    let mut dispatch_kind: Option<AuthorDispatchKind> = None;
    let mut qa_report_path: Option<PathBuf> = None;
    let mut latest_qa_report_path: Option<PathBuf> = None;
    let mut active_agent_override: Option<String> = None;
    let mut review_skipped = false;
    let mut last_review_dispatch = Value::Null;
    let mut last_review_decision = Value::Null;
    let mut last_skip_update = Value::Null;
    let mut last_structural = Value::Null;
    let mut last_structural_update: Value;

    for _ in 0..max_loops {
        let bump_micro = dispatch_kind == Some(AuthorDispatchKind::MicroCorrection);
        let author_transition = json_step(
            "author_transition",
            transition_active_slice(TransitionActiveSliceOptions {
                queue_path: paths.queue_path.clone(),
                active_state_path: paths.active_state_path.clone(),
                slice_id: current_slice_id.clone(),
                stage: Stage::Author,
                active_agent: active_agent_override.clone(),
                bump_attempts: !bump_micro,
                bump_micro_corrections: bump_micro,
            }),
        )?;
        render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

        let author_dispatch = json_step_value(
            "author_dispatch",
            dispatch_stage_value(DispatchStageOptions {
                workspace_root: paths.workspace_root.clone(),
                queue_path: paths.queue_path.clone(),
                active_state_path: paths.active_state_path.clone(),
                author_output_dir: paths.author_output_dir.clone(),
                dispatch_root: paths.dispatch_root.clone(),
                slicemap_path: paths.slicemap_path.clone(),
                legacy_path: paths.legacy_path.clone(),
                host: options.host,
                qa_report_path: qa_report_path.clone(),
                latest_qa_report_path: latest_qa_report_path.clone(),
                slice_id: current_slice_id.clone(),
                dispatch_kind,
                mutagen_root: options.mutagen_root.clone(),
            }),
        )?;

        let structural_transition = json_step(
            "structural_transition",
            transition_active_slice(TransitionActiveSliceOptions {
                queue_path: paths.queue_path.clone(),
                active_state_path: paths.active_state_path.clone(),
                slice_id: current_slice_id.clone(),
                stage: Stage::StructuralCheck,
                active_agent: None,
                bump_attempts: false,
                bump_micro_corrections: false,
            }),
        )?;
        render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

        last_structural = serde_json::to_value(structural_check(StructuralCheckOptions {
            slice_id: current_slice_id.clone(),
            workspace_root: paths.workspace_root.clone(),
            queue_path: paths.queue_path.clone(),
            author_output_dir: paths.author_output_dir.clone(),
            loc_script_path: PathBuf::from("plugins/mutagen/scripts/slice_loc.sh"),
        }))?;

        if required_string(&last_structural, "verdict")? == "fail" {
            let snapshot = load_active_slice(&paths.active_state_path)?;
            if findings_are_only_traces_to_drift(&last_structural)
                && snapshot.micro_corrections_used < snapshot.max_micro_corrections
            {
                let report = write_structural_citations_qa_report(
                    &paths.workspace_root,
                    current_slice_id,
                    &last_structural,
                )?;
                dispatch_kind = Some(AuthorDispatchKind::MicroCorrection);
                qa_report_path = Some(report.qa_report_path);
                latest_qa_report_path = Some(report.latest_qa_report_path);
                active_agent_override = None;
                continue;
            }

            let structural_reason = join_findings(&last_structural);
            last_structural_update = json_step(
                "structural_queue_update",
                update_slice(UpdateSliceOptions {
                    queue_path: paths.queue_path.clone(),
                    slice_id: current_slice_id.clone(),
                    status: Some(SliceStatus::Escalated),
                    attempts: None,
                    micro_corrections_used: None,
                    karai_structural: Some(KaraiStructuralVerdict::Fail),
                    bishop: None,
                    tiger_claw: None,
                    micro_correction: None,
                    completed_at: None,
                    clear_completed_at: false,
                    escalation_reason: Some(structural_reason),
                    clear_escalation_reason: false,
                    human_check_resolved_at: None,
                    clear_human_check_resolved_at: false,
                }),
            )?;
            render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

            let stop_condition = last_structural
                .get("stop_condition")
                .and_then(Value::as_str)
                .unwrap_or("structural_failure");

            return Ok(RunnerOutcome {
                payload: json!({
                    "ok": true,
                    "status": "escalated",
                    "stage": "structural_check",
                    "slice_id": current_slice_id,
                    "stop_condition": stop_condition,
                    "prepare_next": prepare_output,
                    "author_transition": author_transition,
                    "author_dispatch": author_dispatch,
                    "structural_transition": structural_transition,
                    "structural": last_structural,
                    "structural_queue_update": last_structural_update,
                }),
                exit_code: 0,
            });
        }

        last_structural_update = json_step(
            "structural_queue_update",
            update_slice(UpdateSliceOptions {
                queue_path: paths.queue_path.clone(),
                slice_id: current_slice_id.clone(),
                status: None,
                attempts: None,
                micro_corrections_used: None,
                karai_structural: Some(KaraiStructuralVerdict::Pass),
                bishop: None,
                tiger_claw: None,
                micro_correction: None,
                completed_at: None,
                clear_completed_at: false,
                escalation_reason: None,
                clear_escalation_reason: false,
                human_check_resolved_at: None,
                clear_human_check_resolved_at: false,
            }),
        )?;
        render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

        let active_state = load_active_slice(&paths.active_state_path)?;
        if active_state.pipeline_mode.to_string() == "lightweight" && !active_state.review_required
        {
            review_skipped = true;
            last_skip_update = json_step(
                "review_skip_update",
                update_slice(UpdateSliceOptions {
                    queue_path: paths.queue_path.clone(),
                    slice_id: current_slice_id.clone(),
                    status: None,
                    attempts: None,
                    micro_corrections_used: None,
                    karai_structural: None,
                    bishop: Some(BishopVerdict::Skip),
                    tiger_claw: Some(TigerClawVerdict::Skip),
                    micro_correction: None,
                    completed_at: None,
                    clear_completed_at: false,
                    escalation_reason: None,
                    clear_escalation_reason: false,
                    human_check_resolved_at: None,
                    clear_human_check_resolved_at: false,
                }),
            )?;
            render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;
            break;
        }

        let review_transition = json_step(
            "review_transition",
            transition_active_slice(TransitionActiveSliceOptions {
                queue_path: paths.queue_path.clone(),
                active_state_path: paths.active_state_path.clone(),
                slice_id: current_slice_id.clone(),
                stage: Stage::Review,
                active_agent: None,
                bump_attempts: false,
                bump_micro_corrections: false,
            }),
        )?;
        render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

        last_review_dispatch = json_step_value(
            "review_dispatch",
            dispatch_stage_value(DispatchStageOptions {
                workspace_root: paths.workspace_root.clone(),
                queue_path: paths.queue_path.clone(),
                active_state_path: paths.active_state_path.clone(),
                author_output_dir: paths.author_output_dir.clone(),
                dispatch_root: paths.dispatch_root.clone(),
                slicemap_path: paths.slicemap_path.clone(),
                legacy_path: paths.legacy_path.clone(),
                host: options.host,
                qa_report_path: None,
                latest_qa_report_path: None,
                slice_id: current_slice_id.clone(),
                dispatch_kind: None,
                mutagen_root: options.mutagen_root.clone(),
            }),
        )?;

        last_review_decision = json_step(
            "review_decision",
            review_decision(ReviewDecisionOptions {
                workspace_root: paths.workspace_root.clone(),
                queue_path: paths.queue_path.clone(),
                active_state_path: paths.active_state_path.clone(),
                qa_report_path: None,
                latest_qa_report_path: None,
                slice_id: current_slice_id.clone(),
            }),
        )?;
        render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

        match required_string(&last_review_decision, "action")?.as_str() {
            "continue" => break,
            "micro_correction" => {
                dispatch_kind = Some(AuthorDispatchKind::MicroCorrection);
                qa_report_path = Some(PathBuf::from(required_string(
                    &last_review_decision,
                    "qa_report_path",
                )?));
                latest_qa_report_path = Some(PathBuf::from(required_string(
                    &last_review_decision,
                    "latest_qa_report_path",
                )?));
                active_agent_override =
                    Some(required_string(&last_review_decision, "active_agent")?);
                continue;
            }
            "retry" => {
                dispatch_kind = Some(AuthorDispatchKind::Retry);
                qa_report_path = Some(PathBuf::from(required_string(
                    &last_review_decision,
                    "qa_report_path",
                )?));
                latest_qa_report_path = Some(PathBuf::from(required_string(
                    &last_review_decision,
                    "latest_qa_report_path",
                )?));
                active_agent_override = None;
                continue;
            }
            "escalated" => {
                let stop_condition = last_review_decision
                    .get("stop_condition")
                    .and_then(Value::as_str)
                    .unwrap_or("retry_budget_exhausted");
                return Ok(RunnerOutcome {
                    payload: json!({
                        "ok": true,
                        "status": "escalated",
                        "stage": "review",
                        "slice_id": current_slice_id,
                        "stop_condition": stop_condition,
                        "prepare_next": prepare_output,
                        "author_transition": author_transition,
                        "author_dispatch": author_dispatch,
                        "structural_transition": structural_transition,
                        "structural": last_structural,
                        "structural_queue_update": last_structural_update,
                        "review_transition": review_transition,
                        "review_dispatch": last_review_dispatch,
                        "review_decision": last_review_decision,
                    }),
                    exit_code: 0,
                });
            }
            other => bail!(StepFailure::new(
                "review_decision",
                format!("review-decision returned unsupported action '{other}'"),
            )),
        }
    }

    if last_structural.is_null() {
        bail!(StepFailure::new(
            "loop_guard",
            format!("slice runner exceeded its retry guard while processing '{current_slice_id}'"),
        ));
    }

    let state_record_transition = json_step(
        "state_record_transition",
        transition_active_slice(TransitionActiveSliceOptions {
            queue_path: paths.queue_path.clone(),
            active_state_path: paths.active_state_path.clone(),
            slice_id: current_slice_id.clone(),
            stage: Stage::StateRecord,
            active_agent: None,
            bump_attempts: false,
            bump_micro_corrections: false,
        }),
    )?;
    render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

    let finalize = json_step(
        "finalize_slice",
        finalize_slice(FinalizeSliceOptions {
            workspace_root: paths.workspace_root.clone(),
            queue_path: paths.queue_path.clone(),
            active_state_path: paths.active_state_path.clone(),
            dispatch_log_path: paths.dispatch_log_path.clone(),
            summary_root: paths.summary_root.clone(),
            slice_id: current_slice_id.clone(),
            completed_at: utc_timestamp()?,
            accept_missing_artifacts: false,
            accept_broken_build: false,
        }),
    )?;
    render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

    Ok(RunnerOutcome {
        payload: json!({
            "ok": true,
            "status": "completed",
            "slice_id": current_slice_id,
            "review_skipped": review_skipped,
            "prepare_next": prepare_output,
            "structural": last_structural,
            "review_dispatch": last_review_dispatch,
            "review_decision": last_review_decision,
            "review_skip_update": last_skip_update,
            "state_record_transition": state_record_transition,
            "finalize": finalize,
        }),
        exit_code: 0,
    })
}

fn run_cohort_once_inner(options: RunCohortOnceOptions) -> Result<RunnerOutcome> {
    let paths = RunnerPaths::resolve(
        options.workspace_root.clone(),
        options.queue_path.clone(),
        options.queue_validation_path.clone(),
        options.workflow_config_path.clone(),
        options.active_state_path.clone(),
        options.author_output_dir.clone(),
        options.dispatch_root.clone(),
        options.dispatch_log_path.clone(),
        options.summary_root.clone(),
        options.slicemap_path.clone(),
        options.legacy_path.clone(),
    )?;

    ensure_no_active_slice(&paths.active_state_path, "cohort")?;

    if let QueueReadyResult::NotReady(outcome) =
        queue_ready(&paths.queue_path, &paths.queue_validation_path)?
    {
        return Ok(RunnerOutcome {
            payload: json!({
                "ok": false,
                "status": "queue_validation_failed",
                "mode": "prepare_cohort",
                "completed_count": 0,
                "completed_slices": [],
                "completion_markers": [],
                "terminal": outcome.payload,
            }),
            exit_code: 2,
        });
    }

    let prepare_cohort_output = json_step(
        "prepare_cohort",
        prepare_cohort(PrepareCohortOptions {
            workspace_root: paths.workspace_root.clone(),
            queue_path: paths.queue_path.clone(),
            queue_validation_path: paths.queue_validation_path.clone(),
            workflow_config_path: paths.workflow_config_path.clone(),
            host: options.host,
            dry_run: false,
        }),
    )?;

    match status_of(&prepare_cohort_output).unwrap_or_default() {
        "queue_clear" | "stalled" => {
            return Ok(RunnerOutcome {
                payload: json!({
                    "ok": true,
                    "status": status_of(&prepare_cohort_output).unwrap_or_default(),
                    "mode": "prepare_cohort",
                    "completed_count": 0,
                    "completed_slices": [],
                    "completion_markers": [],
                    "prepare_cohort": prepare_cohort_output,
                    "terminal": prepare_cohort_output,
                }),
                exit_code: 0,
            });
        }
        "serial_only" => {
            let serial = run_slice_once(RunSliceOnceOptions {
                workspace_root: options.workspace_root,
                queue_path: options.queue_path,
                queue_validation_path: options.queue_validation_path,
                workflow_config_path: options.workflow_config_path,
                active_state_path: options.active_state_path,
                author_output_dir: options.author_output_dir,
                dispatch_root: options.dispatch_root,
                dispatch_log_path: options.dispatch_log_path,
                summary_root: options.summary_root,
                slicemap_path: options.slicemap_path,
                legacy_path: options.legacy_path,
                host: options.host,
                slice_id: None,
                mutagen_root: options.mutagen_root,
            });
            return Ok(normalize_serial_result(prepare_cohort_output, serial));
        }
        "ready" => {}
        other => bail!(StepFailure::new(
            "prepare_cohort",
            format!("prepare-cohort returned unsupported status '{other}'"),
        )),
    }

    let cohort = prepare_cohort_output
        .get("cohort")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if cohort.len() <= 1 {
        let selected_slice_id = cohort
            .first()
            .and_then(|member| member.get("slice_id"))
            .and_then(Value::as_str)
            .context("prepare-cohort omitted cohort[0].slice_id")?
            .to_string();

        let serial = run_slice_once(RunSliceOnceOptions {
            workspace_root: options.workspace_root,
            queue_path: options.queue_path,
            queue_validation_path: options.queue_validation_path,
            workflow_config_path: options.workflow_config_path,
            active_state_path: options.active_state_path,
            author_output_dir: options.author_output_dir,
            dispatch_root: options.dispatch_root,
            dispatch_log_path: options.dispatch_log_path,
            summary_root: options.summary_root,
            slicemap_path: options.slicemap_path,
            legacy_path: options.legacy_path,
            host: options.host,
            slice_id: Some(selected_slice_id),
            mutagen_root: options.mutagen_root,
        });
        return Ok(normalize_serial_result(prepare_cohort_output, serial));
    }

    let slice_ids = cohort
        .iter()
        .filter_map(|member| member.get("slice_id").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();

    let materialized = materialize_cohort_worktrees(MaterializeCohortWorktreesOptions {
        workspace_root: paths.workspace_root.clone(),
        slice_ids,
    })
    .context("worktree_create_failed")?;

    let cleanup_guard = WorktreeCleanupGuard::new(
        paths.workspace_root.clone(),
        PathBuf::from(&materialized.worktree_root),
    );

    let dispatched = dispatch_cohort_members_native(
        &paths.workspace_root,
        &materialized,
        options.host,
        options.mutagen_root.as_deref(),
    )
    .context("cohort_member_failed")?;

    let member_json = dispatched
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let applied = apply_cohort_dispatch(ApplyCohortDispatchOptions {
        workspace_root: paths.workspace_root.clone(),
        queue_path: paths.queue_path.clone(),
        dispatch_log_path: paths.dispatch_log_path.clone(),
        member_json,
    })
    .context("apply_cohort_dispatch_failed")?;
    render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

    drop(cleanup_guard);

    Ok(cohort_apply_outcome(prepare_cohort_output, applied))
}

fn run_execute_next_inner(options: RunExecuteNextOptions) -> Result<RunnerOutcome> {
    let mut completed_slices: Vec<Value> = Vec::new();
    let max_loops = options.max_loops.max(1);

    for _ in 0..max_loops {
        let run_output = run_cohort_once(RunCohortOnceOptions {
            workspace_root: options.workspace_root.clone(),
            queue_path: options.queue_path.clone(),
            queue_validation_path: options.queue_validation_path.clone(),
            workflow_config_path: options.workflow_config_path.clone(),
            active_state_path: options.active_state_path.clone(),
            author_output_dir: options.author_output_dir.clone(),
            dispatch_root: options.dispatch_root.clone(),
            dispatch_log_path: options.dispatch_log_path.clone(),
            summary_root: options.summary_root.clone(),
            slicemap_path: options.slicemap_path.clone(),
            legacy_path: options.legacy_path.clone(),
            host: options.host,
            mutagen_root: options.mutagen_root.clone(),
        });

        if run_output.exit_code == 2 {
            completed_slices.extend(completed_entries(&run_output.payload));
            return Ok(RunnerOutcome {
                payload: json!({
                    "ok": false,
                    "status": "queue_validation_failed",
                    "completed_count": completed_slices.len(),
                    "completed_slices": completed_slices,
                    "completion_markers": completion_markers_from_values(&completed_slices),
                    "terminal": run_output.payload,
                }),
                exit_code: 2,
            });
        }

        if run_output.exit_code != 0 {
            return Ok(failure_outcome(
                "run_cohort_once_failed",
                &run_output.payload.to_string(),
                json!({}),
                1,
            ));
        }

        match status_of(&run_output.payload).unwrap_or_default() {
            "completed" => {
                completed_slices.extend(completed_entries(&run_output.payload));
            }
            "queue_clear" | "stalled" | "escalated" => {
                completed_slices.extend(completed_entries(&run_output.payload));
                let status = status_of(&run_output.payload).unwrap_or_default();
                return Ok(RunnerOutcome {
                    payload: json!({
                        "ok": true,
                        "status": status,
                        "completed_count": completed_slices.len(),
                        "completed_slices": completed_slices,
                        "completion_markers": completion_markers_from_values(&completed_slices),
                        "terminal": run_output.payload,
                    }),
                    exit_code: 0,
                });
            }
            other => {
                return Ok(failure_outcome(
                    "run_cohort_once_failed",
                    &format!("run-cohort-once returned unsupported status `{other}`"),
                    json!({}),
                    1,
                ));
            }
        }
    }

    Ok(failure_outcome(
        "loop_guard_exceeded",
        "execute-next runner exceeded its loop guard",
        json!({}),
        1,
    ))
}

fn run_continue_slice_inner(
    options: ContinueSliceOptions,
    current_slice_id: &mut String,
) -> Result<RunnerOutcome> {
    let paths = RunnerPaths::resolve(
        options.workspace_root,
        options.queue_path,
        options.queue_validation_path,
        options.workflow_config_path,
        options.active_state_path,
        options.author_output_dir,
        options.dispatch_root,
        options.dispatch_log_path,
        options.summary_root,
        options.slicemap_path,
        options.legacy_path,
    )?;
    ensure_workspace_dirs(&paths.workspace_root)?;

    if !paths.active_state_path.is_file() {
        bail!(StepFailure::new(
            "no_active_slice",
            format!(
                "no active-slice.json at {}; nothing to continue. Use run-slice-once or prepare-next first.",
                display_path(&paths.active_state_path)
            ),
        ));
    }

    let active_state = load_active_slice(&paths.active_state_path)?;
    *current_slice_id = active_state.slice_id.clone();

    if let Some(requested) = options.slice_id.as_ref()
        && requested != &active_state.slice_id
    {
        bail!(StepFailure::new(
            "active_slice_mismatch",
            format!(
                "active slice is '{}', not '{}'. Resolve or clear the active slice before continuing a different one.",
                active_state.slice_id, requested
            ),
        ));
    }

    let queue = load_queue_file(&paths.queue_path)?;
    let queue_status = queue
        .slices
        .iter()
        .find(|candidate| candidate.id == active_state.slice_id)
        .map(|row| row.status)
        .with_context(|| format!("slice '{}' not found in queue", active_state.slice_id))?;
    if matches!(
        queue_status,
        SliceStatus::Completed
            | SliceStatus::Escalated
            | SliceStatus::Refused
            | SliceStatus::FinalizationFailed
    ) {
        bail!(StepFailure::new(
            "slice_already_terminal",
            format!(
                "slice '{}' is already at status '{:?}'; continue-slice is not a do-over. Clear and re-queue the slice instead.",
                active_state.slice_id, queue_status
            ),
        ));
    }

    if matches!(active_state.stage, Stage::Author) {
        bail!(StepFailure::new(
            "not_resumable_at_author",
            format!(
                "active slice '{}' is at stage 'author'; continue-slice resumes from structural-check onward. Use run-slice-once or resume-slice --from-stage structural-check.",
                active_state.slice_id,
            ),
        ));
    }

    let mut steps: serde_json::Map<String, Value> = serde_json::Map::new();

    if matches!(active_state.stage, Stage::StructuralCheck) {
        let report = run_structural_step(&paths, current_slice_id)?;
        steps.insert("structural_transition".into(), report.transition.clone());
        steps.insert("structural".into(), report.structural.clone());
        steps.insert(
            "structural_queue_update".into(),
            report.queue_update.clone(),
        );
        if report.failed {
            return Ok(RunnerOutcome {
                payload: json!({
                    "ok": true,
                    "status": "escalated",
                    "stage": "structural_check",
                    "slice_id": current_slice_id,
                    "stop_condition": report.stop_condition,
                    "structural_transition": report.transition,
                    "structural": report.structural,
                    "structural_queue_update": report.queue_update,
                }),
                exit_code: 0,
            });
        }
    }

    let mut review_skipped = false;
    if matches!(active_state.stage, Stage::StructuralCheck | Stage::Review) {
        let post = load_active_slice(&paths.active_state_path)?;
        let lightweight_skip =
            post.pipeline_mode.to_string() == "lightweight" && !post.review_required;

        if lightweight_skip {
            review_skipped = true;
            let skip_update = json_step(
                "review_skip_update",
                update_slice(UpdateSliceOptions {
                    queue_path: paths.queue_path.clone(),
                    slice_id: current_slice_id.clone(),
                    status: None,
                    attempts: None,
                    micro_corrections_used: None,
                    karai_structural: None,
                    bishop: Some(BishopVerdict::Skip),
                    tiger_claw: Some(TigerClawVerdict::Skip),
                    micro_correction: None,
                    completed_at: None,
                    clear_completed_at: false,
                    escalation_reason: None,
                    clear_escalation_reason: false,
                    human_check_resolved_at: None,
                    clear_human_check_resolved_at: false,
                }),
            )?;
            render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;
            steps.insert("review_skip_update".into(), skip_update);
        } else {
            let report = run_review_step(
                &paths,
                current_slice_id,
                options.host,
                options.mutagen_root.as_ref(),
            )?;
            steps.insert("review_transition".into(), report.transition.clone());
            steps.insert("review_dispatch".into(), report.dispatch.clone());
            steps.insert("review_decision".into(), report.decision.clone());

            match report.action.as_str() {
                "continue" => {}
                "escalated" => {
                    return Ok(RunnerOutcome {
                        payload: json!({
                            "ok": true,
                            "status": "escalated",
                            "stage": "review",
                            "slice_id": current_slice_id,
                            "stop_condition": report.stop_condition,
                            "review_transition": report.transition,
                            "review_dispatch": report.dispatch,
                            "review_decision": report.decision,
                        }),
                        exit_code: 0,
                    });
                }
                "retry" | "micro_correction" => {
                    bail!(StepFailure::new(
                        "requires_author_redispatch",
                        format!(
                            "review-decision returned '{}'; continue-slice does not re-enter author dispatch. Use run-slice-once instead.",
                            report.action
                        ),
                    ));
                }
                other => bail!(StepFailure::new(
                    "review_decision",
                    format!("review-decision returned unsupported action '{other}'"),
                )),
            }
        }
    }

    let state_record_transition = json_step(
        "state_record_transition",
        transition_active_slice(TransitionActiveSliceOptions {
            queue_path: paths.queue_path.clone(),
            active_state_path: paths.active_state_path.clone(),
            slice_id: current_slice_id.clone(),
            stage: Stage::StateRecord,
            active_agent: None,
            bump_attempts: false,
            bump_micro_corrections: false,
        }),
    )?;
    render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

    let finalize = json_step(
        "finalize_slice",
        finalize_slice(FinalizeSliceOptions {
            workspace_root: paths.workspace_root.clone(),
            queue_path: paths.queue_path.clone(),
            active_state_path: paths.active_state_path.clone(),
            dispatch_log_path: paths.dispatch_log_path.clone(),
            summary_root: paths.summary_root.clone(),
            slice_id: current_slice_id.clone(),
            completed_at: utc_timestamp()?,
            accept_missing_artifacts: false,
            accept_broken_build: false,
        }),
    )?;
    render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

    let mut payload = json!({
        "ok": true,
        "status": "completed",
        "slice_id": current_slice_id,
        "review_skipped": review_skipped,
        "state_record_transition": state_record_transition,
        "finalize": finalize,
    });
    merge_object(&mut payload, Value::Object(steps));

    Ok(RunnerOutcome {
        payload,
        exit_code: 0,
    })
}

struct StructuralStepReport {
    transition: Value,
    structural: Value,
    queue_update: Value,
    failed: bool,
    stop_condition: String,
}

fn run_structural_step(paths: &RunnerPaths, slice_id: &str) -> Result<StructuralStepReport> {
    let transition = json_step(
        "structural_transition",
        transition_active_slice(TransitionActiveSliceOptions {
            queue_path: paths.queue_path.clone(),
            active_state_path: paths.active_state_path.clone(),
            slice_id: slice_id.to_string(),
            stage: Stage::StructuralCheck,
            active_agent: None,
            bump_attempts: false,
            bump_micro_corrections: false,
        }),
    )?;
    render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

    let structural = serde_json::to_value(structural_check(StructuralCheckOptions {
        slice_id: slice_id.to_string(),
        workspace_root: paths.workspace_root.clone(),
        queue_path: paths.queue_path.clone(),
        author_output_dir: paths.author_output_dir.clone(),
        loc_script_path: PathBuf::from("plugins/mutagen/scripts/slice_loc.sh"),
    }))?;

    if required_string(&structural, "verdict")? == "fail" {
        let reason = join_findings(&structural);
        let queue_update = json_step(
            "structural_queue_update",
            update_slice(UpdateSliceOptions {
                queue_path: paths.queue_path.clone(),
                slice_id: slice_id.to_string(),
                status: Some(SliceStatus::Escalated),
                attempts: None,
                micro_corrections_used: None,
                karai_structural: Some(KaraiStructuralVerdict::Fail),
                bishop: None,
                tiger_claw: None,
                micro_correction: None,
                completed_at: None,
                clear_completed_at: false,
                escalation_reason: Some(reason),
                clear_escalation_reason: false,
                human_check_resolved_at: None,
                clear_human_check_resolved_at: false,
            }),
        )?;
        render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;
        let stop_condition = structural
            .get("stop_condition")
            .and_then(Value::as_str)
            .unwrap_or("structural_failure")
            .to_string();
        return Ok(StructuralStepReport {
            transition,
            structural,
            queue_update,
            failed: true,
            stop_condition,
        });
    }

    let queue_update = json_step(
        "structural_queue_update",
        update_slice(UpdateSliceOptions {
            queue_path: paths.queue_path.clone(),
            slice_id: slice_id.to_string(),
            status: None,
            attempts: None,
            micro_corrections_used: None,
            karai_structural: Some(KaraiStructuralVerdict::Pass),
            bishop: None,
            tiger_claw: None,
            micro_correction: None,
            completed_at: None,
            clear_completed_at: false,
            escalation_reason: None,
            clear_escalation_reason: false,
            human_check_resolved_at: None,
            clear_human_check_resolved_at: false,
        }),
    )?;
    render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

    Ok(StructuralStepReport {
        transition,
        structural,
        queue_update,
        failed: false,
        stop_condition: String::new(),
    })
}

struct ReviewStepReport {
    transition: Value,
    dispatch: Value,
    decision: Value,
    action: String,
    stop_condition: String,
}

fn run_review_step(
    paths: &RunnerPaths,
    slice_id: &str,
    host: HostKind,
    mutagen_root: Option<&PathBuf>,
) -> Result<ReviewStepReport> {
    let transition = json_step(
        "review_transition",
        transition_active_slice(TransitionActiveSliceOptions {
            queue_path: paths.queue_path.clone(),
            active_state_path: paths.active_state_path.clone(),
            slice_id: slice_id.to_string(),
            stage: Stage::Review,
            active_agent: None,
            bump_attempts: false,
            bump_micro_corrections: false,
        }),
    )?;
    render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

    let dispatch = json_step_value(
        "review_dispatch",
        dispatch_stage_value(DispatchStageOptions {
            workspace_root: paths.workspace_root.clone(),
            queue_path: paths.queue_path.clone(),
            active_state_path: paths.active_state_path.clone(),
            author_output_dir: paths.author_output_dir.clone(),
            dispatch_root: paths.dispatch_root.clone(),
            slicemap_path: paths.slicemap_path.clone(),
            legacy_path: paths.legacy_path.clone(),
            host,
            qa_report_path: None,
            latest_qa_report_path: None,
            slice_id: slice_id.to_string(),
            dispatch_kind: None,
            mutagen_root: mutagen_root.cloned(),
        }),
    )?;

    let decision = json_step(
        "review_decision",
        review_decision(ReviewDecisionOptions {
            workspace_root: paths.workspace_root.clone(),
            queue_path: paths.queue_path.clone(),
            active_state_path: paths.active_state_path.clone(),
            qa_report_path: None,
            latest_qa_report_path: None,
            slice_id: slice_id.to_string(),
        }),
    )?;
    render_queue_views(&paths.queue_path, &paths.slicemap_path, &paths.legacy_path)?;

    let action = required_string(&decision, "action")?;
    let stop_condition = decision
        .get("stop_condition")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Ok(ReviewStepReport {
        transition,
        dispatch,
        decision,
        action,
        stop_condition,
    })
}

fn dispatch_stage_value(options: DispatchStageOptions) -> Result<Value> {
    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let queue_path = resolve_workspace_path(&workspace_root, &options.queue_path);
    let active_state_path = resolve_workspace_path(&workspace_root, &options.active_state_path);
    let author_output_dir = resolve_workspace_path(&workspace_root, &options.author_output_dir);
    let dispatch_root = resolve_workspace_path(&workspace_root, &options.dispatch_root);
    let slicemap_path = resolve_workspace_path(&workspace_root, &options.slicemap_path);
    let legacy_path = resolve_workspace_path(&workspace_root, &options.legacy_path);

    let prepared = prepare_dispatch(PrepareDispatchOptions {
        workspace_root: workspace_root.clone(),
        queue_path: queue_path.clone(),
        active_state_path: active_state_path.clone(),
        author_output_dir: author_output_dir.clone(),
        dispatch_root,
        qa_report_path: options.qa_report_path.clone(),
        latest_qa_report_path: options.latest_qa_report_path.clone(),
        slice_id: options.slice_id.clone(),
        dispatch_kind: options.dispatch_kind,
    })?;
    let prepared_value = serde_json::to_value(&prepared)?;

    let agent_name = prepared.agent.clone();
    let prompt_path = PathBuf::from(&prepared.prompt_path);
    let stdout_capture_path = PathBuf::from(&prepared.stdout_capture_path);
    let stage_name = serde_json::to_value(prepared.stage)?
        .as_str()
        .unwrap_or("unknown")
        .to_string();

    if let Some(parent) = stdout_capture_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", display_path(parent)))?;
    }

    let prompt = fs::read_to_string(&prompt_path)
        .with_context(|| format!("failed to read {}", display_path(&prompt_path)))?;
    let mutagen_root = resolve_mutagen_root(options.mutagen_root.as_deref(), &workspace_root)?;
    let agent_status = launch_agent(
        &workspace_root,
        &mutagen_root,
        options.host,
        &agent_name,
        &prompt,
        &stdout_capture_path,
    )?;

    if !agent_status.success() {
        bail!(
            json!({
                "ok": false,
                "reason": "agent_dispatch_failed",
                "stage": stage_name,
                "agent": agent_name,
                "stdout_capture_path": display_path(&stdout_capture_path),
                "prepared": prepared_value,
            })
            .to_string()
        );
    }

    let missing_artifacts = prepared
        .required_written_artifacts
        .iter()
        .filter(|artifact| !Path::new(artifact).is_file())
        .cloned()
        .collect::<Vec<_>>();

    if !missing_artifacts.is_empty() {
        bail!(
            json!({
                "ok": false,
                "reason": "required_artifacts_missing",
                "stage": stage_name,
                "agent": agent_name,
                "stdout_capture_path": display_path(&stdout_capture_path),
                "missing_artifacts": missing_artifacts,
                "prepared": prepared_value,
            })
            .to_string()
        );
    }

    let mut review_record = Value::Null;
    if stage_name == "review" {
        let qa_report_path = prepared
            .qa_report_path
            .as_deref()
            .context("prepare-dispatch omitted review report path")?;
        let latest_qa_report_path = prepared
            .latest_qa_report_path
            .as_deref()
            .context("prepare-dispatch omitted latest review report path")?;

        review_record = serde_json::to_value(record_review_verdict(RecordReviewVerdictOptions {
            workspace_root: workspace_root.clone(),
            queue_path,
            active_state_path,
            qa_report_path: Some(PathBuf::from(qa_report_path)),
            latest_qa_report_path: Some(PathBuf::from(latest_qa_report_path)),
            slice_id: options.slice_id,
        })?)?;
        render_queue_views(
            &resolve_workspace_path(&workspace_root, &options.queue_path),
            &slicemap_path,
            &legacy_path,
        )?;
    }

    Ok(json!({
        "ok": true,
        "stage": stage_name,
        "agent": agent_name,
        "stdout_capture_path": display_path(&stdout_capture_path),
        "review_record": review_record,
        "prepared": prepared_value,
    }))
}

fn dispatch_cohort_members_native(
    workspace_root: &Path,
    materialized: &MaterializeCohortWorktreesResult,
    host: HostKind,
    mutagen_root: Option<&Path>,
) -> Result<Vec<DispatchedCohortMember>> {
    let harness_bin = resolve_harness_binary()?;
    let mut children = Vec::new();

    for member in &materialized.members {
        let result_path = PathBuf::from(&member.result_path);
        let status_path = PathBuf::from(&member.status_path);
        let worktree_path = PathBuf::from(&member.worktree_path);

        if let Some(parent) = result_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", display_path(parent)))?;
        }
        if let Some(parent) = status_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", display_path(parent)))?;
        }

        let result_file = File::create(&result_path)
            .with_context(|| format!("failed to create {}", display_path(&result_path)))?;
        let stderr_file = result_file
            .try_clone()
            .with_context(|| format!("failed to clone {}", display_path(&result_path)))?;

        let mut command = Command::new(&harness_bin);
        command
            .arg("run-slice-once")
            .arg("--workspace-root")
            .arg(display_path(&worktree_path))
            .arg("--queue")
            .arg(display_path(&worktree_path.join("slices/queue.json")))
            .arg("--queue-validation")
            .arg(display_path(
                &worktree_path.join(".mutagen/state/queue-validation.json"),
            ))
            .arg("--workflow-config")
            .arg(display_path(&worktree_path.join(".claude/workflow.json")))
            .arg("--active-state")
            .arg(display_path(
                &worktree_path.join(".mutagen/state/active-slice.json"),
            ))
            .arg("--author-output-dir")
            .arg(display_path(
                &worktree_path.join(".mutagen/state/author-output"),
            ))
            .arg("--dispatch-root")
            .arg(display_path(&worktree_path.join(".mutagen/state/dispatch")))
            .arg("--dispatch-log")
            .arg(display_path(
                &worktree_path.join(".mutagen/state/dispatch-log.jsonl"),
            ))
            .arg("--summary-root")
            .arg(display_path(&worktree_path.join("slices")))
            .arg("--slicemap")
            .arg(display_path(&worktree_path.join("slices/slicemap.md")))
            .arg("--legacy")
            .arg(display_path(&worktree_path.join("slices/queue.md")))
            .arg("--host")
            .arg(host_kind_name(host))
            .arg("--slice-id")
            .arg(&member.slice_id)
            .stdout(Stdio::from(result_file))
            .stderr(Stdio::from(stderr_file));

        if let Some(root) = mutagen_root {
            command
                .arg("--mutagen-root")
                .arg(display_path(root))
                .env("MUTAGEN_ROOT", root);
        }

        let child = command.spawn().with_context(|| {
            format!(
                "failed to spawn cohort member `{}` with {}",
                member.slice_id,
                display_path(&harness_bin)
            )
        })?;

        children.push(DispatchChild {
            member: member.clone(),
            worktree_path,
            result_path,
            status_path,
            child,
        });
    }

    let mut members = Vec::new();

    for mut dispatch in children {
        let exit_code = dispatch
            .child
            .wait()
            .with_context(|| format!("failed to wait for `{}`", dispatch.member.slice_id))?
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "1".to_string());

        fs::write(&dispatch.status_path, format!("{exit_code}\n"))
            .with_context(|| format!("failed to write {}", display_path(&dispatch.status_path)))?;

        let outcome = collect_cohort_member_result(CollectCohortMemberResultOptions {
            workspace_root: workspace_root.to_path_buf(),
            worktree_root: dispatch.worktree_path.clone(),
            slice_id: dispatch.member.slice_id.clone(),
            result_path: dispatch.result_path.clone(),
            status_path: dispatch.status_path.clone(),
        })?;

        members.push(DispatchedCohortMember {
            slice_id: dispatch.member.slice_id,
            worktree_path: display_path(&dispatch.worktree_path),
            result_path: display_path(&dispatch.result_path),
            status_path: display_path(&dispatch.status_path),
            outcome,
        });
    }

    Ok(members)
}

fn normalize_serial_result(
    prepare_cohort_output: Value,
    run_output: RunnerOutcome,
) -> RunnerOutcome {
    if run_output.exit_code == 2 {
        return RunnerOutcome {
            payload: json!({
                "ok": false,
                "status": "queue_validation_failed",
                "mode": "serial_only",
                "completed_count": 0,
                "completed_slices": [],
                "completion_markers": [],
                "prepare_cohort": prepare_cohort_output,
                "terminal": run_output.payload,
            }),
            exit_code: 2,
        };
    }

    if run_output.exit_code != 0 {
        return failure_outcome(
            "run_slice_once_failed",
            &run_output.payload.to_string(),
            json!({}),
            1,
        );
    }

    match status_of(&run_output.payload).unwrap_or_default() {
        "completed" => {
            let completed_entry = json!({
                "slice_id": run_output.payload.get("slice_id").cloned().unwrap_or(Value::Null),
                "completion_marker": run_output
                    .payload
                    .pointer("/finalize/completion_marker")
                    .cloned()
                    .unwrap_or_else(|| json!("")),
                "review_skipped": run_output
                    .payload
                    .get("review_skipped")
                    .cloned()
                    .unwrap_or(Value::Bool(false)),
                "summary_path": run_output
                    .payload
                    .pointer("/finalize/summary_path")
                    .cloned()
                    .unwrap_or(Value::Null),
                "worktree_path": Value::Null,
            });
            let completed_slices = vec![completed_entry];
            RunnerOutcome {
                payload: json!({
                    "ok": true,
                    "status": "completed",
                    "mode": "serial_only",
                    "completed_count": completed_slices.len(),
                    "completed_slices": completed_slices,
                    "completion_markers": completion_markers_from_values(&completed_slices),
                    "prepare_cohort": prepare_cohort_output,
                    "terminal": run_output.payload,
                }),
                exit_code: 0,
            }
        }
        "queue_clear" | "stalled" | "escalated" => RunnerOutcome {
            payload: json!({
                "ok": true,
                "status": status_of(&run_output.payload).unwrap_or_default(),
                "mode": "serial_only",
                "completed_count": 0,
                "completed_slices": [],
                "completion_markers": [],
                "prepare_cohort": prepare_cohort_output,
                "terminal": run_output.payload,
            }),
            exit_code: 0,
        },
        other => failure_outcome(
            "run_slice_once_failed",
            &format!("run-slice-once returned unsupported status `{other}`"),
            json!({}),
            1,
        ),
    }
}

fn cohort_apply_outcome(
    prepare_cohort_output: Value,
    applied: ApplyCohortDispatchResult,
) -> RunnerOutcome {
    match applied {
        ApplyCohortDispatchResult::Completed {
            completed_count,
            completed_slices,
            completion_markers,
        } => RunnerOutcome {
            payload: json!({
                "ok": true,
                "status": "completed",
                "mode": "bounded_cohort",
                "cohort_size": completed_count,
                "completed_count": completed_count,
                "completed_slices": completed_slices,
                "completion_markers": completion_markers,
                "prepare_cohort": prepare_cohort_output,
            }),
            exit_code: 0,
        },
        ApplyCohortDispatchResult::Escalated {
            slice_id,
            worktree_path,
            completed_count,
            completed_slices,
            completion_markers,
            terminal,
            stage,
            stop_condition,
            conflicting_slice_id,
            conflicting_path,
        } => {
            let mut payload = json!({
                "ok": true,
                "status": "escalated",
                "slice_id": slice_id,
                "worktree_path": worktree_path,
                "completed_count": completed_count,
                "completed_slices": completed_slices,
                "completion_markers": completion_markers,
                "terminal": terminal,
            });
            insert_optional(&mut payload, "stage", stage);
            insert_optional(&mut payload, "stop_condition", stop_condition);
            insert_optional(&mut payload, "conflicting_slice_id", conflicting_slice_id);
            insert_optional(&mut payload, "conflicting_path", conflicting_path);

            RunnerOutcome {
                payload,
                exit_code: 0,
            }
        }
        ApplyCohortDispatchResult::Failed {
            slice_id,
            worktree_path,
            completed_count,
            completed_slices,
            completion_markers,
            message,
        } => RunnerOutcome {
            payload: json!({
                "ok": false,
                "error": "cohort_member_failed",
                "slice_id": slice_id,
                "worktree_path": worktree_path,
                "completed_count": completed_count,
                "completed_slices": completed_slices,
                "completion_markers": completion_markers,
                "message": message,
            }),
            exit_code: 1,
        },
    }
}

fn queue_ready(queue_path: &Path, queue_validation_path: &Path) -> Result<QueueReadyResult> {
    match check_queue_readiness(queue_path, queue_validation_path)? {
        QueueReadiness::Ready { .. } => Ok(QueueReadyResult::Ready),
        QueueReadiness::NotReady { failure } => {
            let mut payload = readiness_failure_value(&failure)?;
            payload["ok"] = json!(false);
            Ok(QueueReadyResult::NotReady(RunnerOutcome {
                payload,
                exit_code: 2,
            }))
        }
    }
}

fn launch_agent(
    workspace_root: &Path,
    mutagen_root: &Path,
    host: HostKind,
    persona: &str,
    prompt: &str,
    stdout_capture_path: &Path,
) -> Result<std::process::ExitStatus> {
    let persona_file = mutagen_root.join("agents").join(format!("{persona}.md"));
    if !persona_file.is_file() {
        bail!("no persona file at {}", display_path(&persona_file));
    }

    let persona_body =
        strip_frontmatter(&fs::read_to_string(&persona_file).with_context(|| {
            format!(
                "failed to read persona file at {}",
                display_path(&persona_file)
            )
        })?);
    let profile = persona.to_ascii_lowercase();
    let framing =
        format!("# You are {persona}\n\n{persona_body}\n\n---\n\n# Current task\n\n{prompt}\n");

    let stdout_file = File::create(stdout_capture_path)
        .with_context(|| format!("failed to create {}", display_path(stdout_capture_path)))?;
    let stderr_file = stdout_file
        .try_clone()
        .with_context(|| format!("failed to clone {}", display_path(stdout_capture_path)))?;

    let mut command = if let Ok(launcher) = env::var("MUTAGEN_AGENT_LAUNCHER") {
        if !launcher.trim().is_empty() {
            let mut command = Command::new(launcher);
            command
                .arg(host_kind_name(host))
                .arg(persona)
                .arg(&profile)
                .arg(framing);
            command
        } else {
            host_command(host, &profile, framing)?
        }
    } else {
        host_command(host, &profile, framing)?
    };

    command
        .current_dir(workspace_root)
        .env("MUTAGEN_ROOT", mutagen_root)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));

    // CodexPro stdin contract — mirrors the matching guard in agent.sh. The
    // Rust dispatch path used to inherit this process's stdin, which (when
    // the harness ran inside a non-TTY Codex shell) made `codex exec` hang on
    // "Reading additional input from stdin...". Mutagen never feeds an agent
    // through stdin — framing carries every byte. Launchers that genuinely
    // want stdin can opt back in via MUTAGEN_AGENT_LAUNCHER_KEEP_STDIN=1.
    if env::var("MUTAGEN_AGENT_LAUNCHER_KEEP_STDIN").as_deref() != Ok("1") {
        command.stdin(Stdio::null());
    }

    command.status().with_context(|| {
        format!(
            "failed to launch {persona} through {}",
            host_kind_name(host)
        )
    })
}

fn host_command(host: HostKind, profile: &str, framing: String) -> Result<Command> {
    match host {
        HostKind::Codex => {
            let bin = env::var("CODEX_BIN").unwrap_or_else(|_| "codex".to_string());
            let mut command = Command::new(bin);
            command
                .arg("exec")
                .arg("--profile")
                .arg(profile)
                .arg("--skip-git-repo-check")
                .arg(framing);
            Ok(command)
        }
        HostKind::Claude => {
            let bin = env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
            let mut command = Command::new(bin);
            command.arg("--print").arg(framing);
            Ok(command)
        }
        HostKind::Stub => bail!(
            "unsupported host 'stub'. Set MUTAGEN_AGENT_LAUNCHER to provide a custom launcher."
        ),
        HostKind::Ollama | HostKind::LmStudio => bail!(
            "host '{:?}' is an inference provider, not an agentic launcher; use `complete-chat` for direct prompting",
            host
        ),
    }
}

fn resolve_mutagen_root(explicit: Option<&Path>, workspace_root: &Path) -> Result<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(path) = explicit {
        candidates.push(path.to_path_buf());
    }
    if let Ok(path) = env::var("MUTAGEN_ROOT")
        && !path.trim().is_empty()
    {
        candidates.push(PathBuf::from(path));
    }
    candidates.push(workspace_root.join("plugins/mutagen"));
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("plugins/mutagen"));
    }
    if let Ok(exe) = env::current_exe()
        && let Some(bin_dir) = exe.parent()
        && let Some(plugin_root) = bin_dir.parent()
    {
        candidates.push(plugin_root.to_path_buf());
    }

    for candidate in candidates {
        if candidate.join("agents").is_dir() {
            return fs::canonicalize(&candidate)
                .with_context(|| format!("failed to resolve {}", display_path(&candidate)));
        }
    }

    bail!(
        "could not resolve MUTAGEN_ROOT; set --mutagen-root or MUTAGEN_ROOT to a plugin root with agents/"
    )
}

fn render_queue_views(queue_path: &Path, slicemap_path: &Path, legacy_path: &Path) -> Result<()> {
    let queue = load_queue_file(queue_path)?;
    let body = render_queue_markdown(&queue);

    if let Some(parent) = slicemap_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", display_path(parent)))?;
    }
    fs::write(slicemap_path, &body)
        .with_context(|| format!("failed to write {}", display_path(slicemap_path)))?;

    if legacy_path != slicemap_path {
        if let Some(parent) = legacy_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", display_path(parent)))?;
        }
        fs::write(legacy_path, body)
            .with_context(|| format!("failed to write {}", display_path(legacy_path)))?;
    }

    Ok(())
}

fn render_queue_markdown(queue: &SliceQueue) -> String {
    let mut slices = queue.slices.iter().collect::<Vec<_>>();
    slices.sort_by(|a, b| a.layer.cmp(&b.layer).then_with(|| a.id.cmp(&b.id)));

    let mut body = String::new();
    body.push_str("# Slice Map\n\n");
    body.push_str(&format!(
        "_Generated: {} - Generated by: {} - Pipeline mode: {} - Schema version: {}_\n\n",
        fallback(&queue.generated_at, "unknown"),
        fallback(&queue.generated_by, "unknown"),
        queue.pipeline_mode,
        queue.version,
    ));
    body.push_str("> This file is a rendering of `slices/queue.json`. The JSON is canonical; this markdown regenerates after every queue mutation.\n\n");
    body.push_str("## Summary\n\n");
    body.push_str(&format!("- **Total:** {}\n", slices.len()));
    body.push_str(&format!(
        "- **By status:** pending: {} - in_progress: {} - completed: {} - blocked_retry: {} - refused: {} - escalated: {}\n",
        count_status(&slices, SliceStatus::Pending),
        count_status(&slices, SliceStatus::InProgress),
        count_status(&slices, SliceStatus::Completed),
        count_status(&slices, SliceStatus::BlockedRetry),
        count_status(&slices, SliceStatus::Refused),
        count_status(&slices, SliceStatus::Escalated),
    ));
    body.push_str(&format!(
        "- **By layer:** L1: {} - L2: {} - L3: {} - L4: {} - L5: {} - L6: {}\n\n",
        count_layer(&slices, 1),
        count_layer(&slices, 2),
        count_layer(&slices, 3),
        count_layer(&slices, 4),
        count_layer(&slices, 5),
        count_layer(&slices, 6),
    ));

    if !queue.planning_advisories.is_empty() {
        body.push_str("## Planning Advisories\n\n");
        for advisory in &queue.planning_advisories {
            body.push_str(&format!(
                "### {}\n\n- **Severity:** {:?}\n- **Summary:** {}\n- **Decision:** {}\n- **User response required:** {}\n- **References:** {}\n- **Affects slices:** {}\n\n",
                fallback(&advisory.id, "(unlabeled advisory)"),
                advisory.severity,
                fallback(&advisory.summary, "-"),
                fallback(&advisory.decision, "-"),
                yesno(advisory.user_response_required),
                joined(&advisory.references),
                joined(&advisory.affects_slices),
            ));
        }
    }

    for layer in 1..=6 {
        let layer_slices = slices
            .iter()
            .copied()
            .filter(|slice| slice.layer == layer)
            .collect::<Vec<_>>();
        if layer_slices.is_empty() {
            continue;
        }

        body.push_str(&format!("## Layer {layer} - {}\n\n", layer_name(layer)));
        for slice in layer_slices {
            body.push_str(&format!(
                "### [Slice ID: {}]{} - {}\n\n",
                slice.id,
                slice
                    .phase
                    .as_ref()
                    .map(|phase| format!(" [Phase: {phase}]"))
                    .unwrap_or_default(),
                fallback(&slice.title, "(untitled)"),
            ));
            body.push_str(&format!(
                "- **Status:** `{}`\n- **Assigned Agent:** {}\n- **Objective:** {}\n- **Bounded Context:** {}\n- **Depends On:** {}\n- **Write Set:** {}\n- **Target LOC:** {}\n- **Context to Update:** `{}`\n- **Review Required:** {}\n- **Attempts:** {} - **Micro-corrections:** {}\n",
                slice_status_name(slice.status),
                fallback(&slice.author_agent, "(unassigned)"),
                fallback(&slice.objective, "-"),
                fallback(&slice.bounded_context, "?"),
                joined(&slice.depends_on),
                joined(&slice.write_set),
                slice.target_loc,
                fallback(&slice.context_to_update, "project_state.md"),
                yesno(slice.review_required),
                slice.attempts,
                slice.micro_corrections_used,
            ));
            body.push_str("- **Traces to:**\n");
            body.push_str(&format!("  - PRD: {}\n", joined(&slice.traces_to.prd)));
            body.push_str(&format!("  - ADR: {}\n", joined(&slice.traces_to.adr)));
            body.push_str(&format!("  - DDD: {}\n", joined(&slice.traces_to.ddd)));
            body.push_str(&format!("  - ISC: {}\n", joined(&slice.traces_to.isc)));
            body.push_str(&format!("  - DSD: {}\n", joined(&slice.traces_to.dsd)));
            body.push_str("- **Implementation Details:**\n");
            for detail in &slice.implementation_details {
                body.push_str(&format!("  - {detail}\n"));
            }
            body.push_str("- **Verification Steps:**\n");
            body.push_str(&format!(
                "  - Acceptance: `{}`\n  - ISC detection: `{}`\n  - DSD conformance: `{}`\n",
                fallback(&slice.verification_steps.acceptance, "-"),
                fallback(&slice.verification_steps.isc_detection, "-"),
                fallback(&slice.verification_steps.dsd_conformance, "-"),
            ));
            if slice.human_check_needed.required {
                body.push_str(&format!(
                    "- **Human Check Needed?:** Yes - {}{}\n",
                    fallback(&slice.human_check_needed.reason, "reason not provided"),
                    slice
                        .human_check_needed
                        .resolved_at
                        .as_ref()
                        .map(|resolved| format!(" - resolved at {resolved}"))
                        .unwrap_or_else(|| " - unresolved".to_string()),
                ));
            } else {
                body.push_str("- **Human Check Needed?:** No\n");
            }
            body.push_str(&format!(
                "- **Verdicts:** karai={} - bishop={} - tiger_claw={}\n",
                option_debug(slice.verdicts.karai_structural),
                option_debug(slice.verdicts.bishop),
                option_debug(slice.verdicts.tiger_claw),
            ));
            if let Some(completed_at) = &slice.completed_at {
                body.push_str(&format!("- **Completed:** {completed_at}\n"));
            }
            if let Some(escalation_reason) = &slice.escalation_reason {
                body.push_str(&format!("- **Escalation:** {escalation_reason}\n"));
            }
            body.push('\n');
        }
    }

    body
}

fn json_step<T: Serialize>(label: &str, result: Result<T>) -> Result<Value> {
    result
        .and_then(|value| serde_json::to_value(value).context("failed to serialize step result"))
        .map_err(|error| StepFailure::new(label, format!("{error:#}")).into())
}

fn json_step_value(label: &str, result: Result<Value>) -> Result<Value> {
    result.map_err(|error| StepFailure::new(label, format!("{error:#}")).into())
}

fn step_error_outcome(slice_id: &str, error: anyhow::Error) -> RunnerOutcome {
    if let Some(step) = error.downcast_ref::<StepFailure>() {
        let mut extra = json!({});
        if !slice_id.is_empty() {
            extra["slice_id"] = json!(slice_id);
        }
        return failure_outcome(&step.error, &step.message, extra, 1);
    }

    failure_outcome("runner_failed", &format!("{error:#}"), json!({}), 1)
}

fn failure_outcome(error: &str, message: &str, extra: Value, exit_code: i32) -> RunnerOutcome {
    let mut payload = json!({
        "ok": false,
        "error": error,
        "message": message,
    });
    merge_object(&mut payload, extra);
    RunnerOutcome { payload, exit_code }
}

fn ensure_no_active_slice(active_state_path: &Path, noun: &str) -> Result<()> {
    if !active_state_path.is_file() {
        return Ok(());
    }

    let existing_slice_id = fs::read_to_string(active_state_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| {
            value
                .get("slice_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        });

    let message = if let Some(slice_id) = existing_slice_id {
        format!(
            "active-slice.json already exists for '{}'. Resolve or clear the current slice before starting another {noun}.",
            slice_id
        )
    } else {
        format!(
            "active-slice.json already exists. Resolve or clear the current slice before starting another {noun}."
        )
    };

    bail!(StepFailure::new("active_slice_present", message))
}

fn ensure_workspace_dirs(workspace_root: &Path) -> Result<()> {
    for path in [
        workspace_root.join(".mutagen/state"),
        workspace_root.join(".mutagen/state/evidence"),
        workspace_root.join("reviews"),
        workspace_root.join("slices"),
    ] {
        fs::create_dir_all(&path)
            .with_context(|| format!("failed to create {}", display_path(&path)))?;
    }
    Ok(())
}

fn required_string(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .with_context(|| format!("missing `{key}`"))
}

fn status_of(value: &Value) -> Option<&str> {
    value.get("status").and_then(Value::as_str)
}

struct StructuralCitationReport {
    qa_report_path: PathBuf,
    latest_qa_report_path: PathBuf,
}

fn findings_are_only_traces_to_drift(structural: &Value) -> bool {
    let Some(findings) = structural.get("findings").and_then(Value::as_array) else {
        return false;
    };
    if findings.is_empty() {
        return false;
    }
    findings
        .iter()
        .all(|finding| finding.get("check").and_then(Value::as_str) == Some("traces_to_drift"))
}

fn write_structural_citations_qa_report(
    workspace_root: &Path,
    slice_id: &str,
    structural: &Value,
) -> Result<StructuralCitationReport> {
    let qa_report_path = workspace_root
        .join("reviews")
        .join(slice_id)
        .join("structural-citations.md");
    let latest_qa_report_path =
        workspace_root.join(".mutagen/state/structural-citations-latest.md");

    let mut body = String::new();
    body.push_str("# QA Report: Structural Citation Drift\n\n");
    body.push_str(&format!("- Slice: {slice_id}\n"));
    body.push_str("- Verdict: fail (structural)\n");
    body.push_str("- Check: traces_to_drift\n\n");
    body.push_str("## Suggested Fixes\n\n");
    body.push_str(
        "The author output is missing literal citation strings the structural check requires. \
         Add each missing ID below to the document — prefer naming them in an `## Upstream Citation Map` \
         section or wherever the substance is already discussed. Do not rewrite working sections.\n\n",
    );

    if let Some(findings) = structural.get("findings").and_then(Value::as_array) {
        for (idx, finding) in findings.iter().enumerate() {
            let detail = finding
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or("missing citation");
            body.push_str(&format!("### Suggested Fix {}: {}\n\n", idx + 1, detail));
            body.push_str("- Add the missing ID verbatim. Joined forms like `DSD-621/622` do not satisfy the literal scan; spell each ID out separately.\n\n");
        }
    }

    body.push_str("## Constraints\n\n");
    body.push_str("- Keep the rest of the author output unchanged. This is a bounded mechanical correction.\n");
    body.push_str("- Re-emit the full State Update block as before.\n");

    if let Some(parent) = qa_report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", display_path(parent)))?;
    }
    fs::write(&qa_report_path, &body)
        .with_context(|| format!("failed to write {}", display_path(&qa_report_path)))?;

    if let Some(parent) = latest_qa_report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", display_path(parent)))?;
    }
    fs::write(&latest_qa_report_path, &body)
        .with_context(|| format!("failed to write {}", display_path(&latest_qa_report_path)))?;

    Ok(StructuralCitationReport {
        qa_report_path,
        latest_qa_report_path,
    })
}

fn join_findings(payload: &Value) -> String {
    let joined = payload
        .get("findings")
        .and_then(Value::as_array)
        .map(|findings| {
            findings
                .iter()
                .filter_map(|finding| finding.get("detail").and_then(Value::as_str))
                .filter(|detail| !detail.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .unwrap_or_default();

    if joined.is_empty() {
        "Karai structural fail".to_string()
    } else {
        joined
    }
}

fn completed_entries(value: &Value) -> Vec<Value> {
    value
        .get("completed_slices")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn completion_markers_from_values(values: &[Value]) -> Vec<Value> {
    values
        .iter()
        .map(|value| {
            value
                .get("completion_marker")
                .cloned()
                .unwrap_or_else(|| json!(""))
        })
        .collect()
}

fn insert_optional(payload: &mut Value, key: &str, value: Option<String>) {
    if let Some(value) = value {
        payload[key] = json!(value);
    }
}

fn merge_object(payload: &mut Value, extra: Value) {
    let Some(payload_object) = payload.as_object_mut() else {
        return;
    };
    let Value::Object(extra_object) = extra else {
        return;
    };

    for (key, value) in extra_object {
        payload_object.insert(key, value);
    }
}

fn strip_frontmatter(raw: &str) -> String {
    let mut in_frontmatter = false;
    let mut lines = Vec::new();

    for line in raw.lines() {
        if line.trim() == "---" {
            in_frontmatter = !in_frontmatter;
            continue;
        }

        if !in_frontmatter {
            lines.push(line);
        }
    }

    lines.join("\n")
}

fn resolve_harness_binary() -> Result<PathBuf> {
    if let Ok(path) = env::var("MUTAGEN_HARNESS_BIN")
        && !path.trim().is_empty()
    {
        return Ok(PathBuf::from(path));
    }

    env::current_exe().context("failed to resolve current harness executable")
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
        Ok(env::current_dir()
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

fn utc_timestamp() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format UTC timestamp")
}

fn fallback<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

fn joined(values: &[String]) -> String {
    if values.is_empty() {
        "(none)".to_string()
    } else {
        values.join(", ")
    }
}

fn yesno(value: bool) -> &'static str {
    if value { "Yes" } else { "No" }
}

fn count_status(slices: &[&crate::queue::Slice], status: SliceStatus) -> usize {
    slices.iter().filter(|slice| slice.status == status).count()
}

fn count_layer(slices: &[&crate::queue::Slice], layer: u32) -> usize {
    slices.iter().filter(|slice| slice.layer == layer).count()
}

fn layer_name(layer: u32) -> &'static str {
    match layer {
        1 => "Foundation",
        2 => "Data",
        3 => "Security",
        4 => "Logic",
        5 => "Interface",
        6 => "Features & Release",
        _ => "Layer",
    }
}

fn option_debug<T: fmt::Debug>(value: Option<T>) -> String {
    value
        .map(|value| format!("{value:?}").to_ascii_lowercase())
        .unwrap_or_else(|| "-".to_string())
}

fn slice_status_name(status: SliceStatus) -> &'static str {
    match status {
        SliceStatus::Pending => "pending",
        SliceStatus::InProgress => "in_progress",
        SliceStatus::BlockedRetry => "blocked_retry",
        SliceStatus::Completed => "completed",
        SliceStatus::Escalated => "escalated",
        SliceStatus::Refused => "refused",
        SliceStatus::FinalizationFailed => "finalization_failed",
    }
}

fn host_kind_name(host: HostKind) -> &'static str {
    match host {
        HostKind::Stub => "stub",
        HostKind::Codex => "codex",
        HostKind::Claude => "claude",
        HostKind::Ollama => "ollama",
        HostKind::LmStudio => "lmstudio",
    }
}

#[derive(Debug)]
struct RunnerPaths {
    workspace_root: PathBuf,
    queue_path: PathBuf,
    queue_validation_path: PathBuf,
    workflow_config_path: PathBuf,
    active_state_path: PathBuf,
    author_output_dir: PathBuf,
    dispatch_root: PathBuf,
    dispatch_log_path: PathBuf,
    summary_root: PathBuf,
    slicemap_path: PathBuf,
    legacy_path: PathBuf,
}

impl RunnerPaths {
    #[allow(clippy::too_many_arguments)]
    fn resolve(
        workspace_root: PathBuf,
        queue_path: PathBuf,
        queue_validation_path: PathBuf,
        workflow_config_path: PathBuf,
        active_state_path: PathBuf,
        author_output_dir: PathBuf,
        dispatch_root: PathBuf,
        dispatch_log_path: PathBuf,
        summary_root: PathBuf,
        slicemap_path: PathBuf,
        legacy_path: PathBuf,
    ) -> Result<Self> {
        let workspace_root = resolve_workspace_root(&workspace_root)?;
        Ok(Self {
            queue_path: resolve_workspace_path(&workspace_root, &queue_path),
            queue_validation_path: resolve_workspace_path(&workspace_root, &queue_validation_path),
            workflow_config_path: resolve_workspace_path(&workspace_root, &workflow_config_path),
            active_state_path: resolve_workspace_path(&workspace_root, &active_state_path),
            author_output_dir: resolve_workspace_path(&workspace_root, &author_output_dir),
            dispatch_root: resolve_workspace_path(&workspace_root, &dispatch_root),
            dispatch_log_path: resolve_workspace_path(&workspace_root, &dispatch_log_path),
            summary_root: resolve_workspace_path(&workspace_root, &summary_root),
            slicemap_path: resolve_workspace_path(&workspace_root, &slicemap_path),
            legacy_path: resolve_workspace_path(&workspace_root, &legacy_path),
            workspace_root,
        })
    }
}

enum QueueReadyResult {
    Ready,
    NotReady(RunnerOutcome),
}

#[derive(Debug)]
struct StepFailure {
    error: String,
    message: String,
}

impl StepFailure {
    fn new(label: &str, message: String) -> Self {
        let error = if label.ends_with("_failed")
            || matches!(
                label,
                "active_slice_present"
                    | "no_active_slice"
                    | "active_slice_mismatch"
                    | "not_resumable_at_author"
                    | "requires_author_redispatch"
                    | "slice_already_terminal"
            ) {
            label.to_string()
        } else {
            format!("{label}_failed")
        };
        Self { error, message }
    }
}

impl fmt::Display for StepFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StepFailure {}

struct DispatchChild {
    member: CohortWorktreeMember,
    worktree_path: PathBuf,
    result_path: PathBuf,
    status_path: PathBuf,
    child: std::process::Child,
}

struct WorktreeCleanupGuard {
    workspace_root: PathBuf,
    worktree_root: PathBuf,
    active: bool,
}

impl WorktreeCleanupGuard {
    fn new(workspace_root: PathBuf, worktree_root: PathBuf) -> Self {
        Self {
            workspace_root,
            worktree_root,
            active: true,
        }
    }
}

impl Drop for WorktreeCleanupGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = cleanup_cohort_worktrees(CleanupCohortWorktreesOptions {
                workspace_root: self.workspace_root.clone(),
                worktree_root: self.worktree_root.clone(),
            });
        }
    }
}

#[cfg(all(test, unix))]
mod stdin_dispatch_tests {
    // CodexPro regression coverage. The agent dispatch path historically
    // inherited the harness's stdin, so `codex exec` (and any launcher that
    // honoured the inherited handle) hung on "Reading additional input from
    // stdin..." inside non-TTY Codex shells. These tests pin the closed-stdin
    // policy on the Rust side; agent.sh's matching `</dev/null` is covered by
    // the bash regression script under tests/scripts/.

    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Mutex;

    // Serialize tests that mutate process-global env — Rust's #[test] harness
    // runs tests in parallel by default and MUTAGEN_AGENT_LAUNCHER is read by
    // launch_agent at call time. One Mutex per env-mutating test is plenty.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn write_launcher_script(dir: &Path, body: &str) -> PathBuf {
        let script = dir.join("launcher.sh");
        fs::write(&script, body).expect("launcher write");
        let mut perms = fs::metadata(&script).expect("launcher stat").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("launcher chmod");
        script
    }

    fn make_persona_workspace() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mutagen_root = dir.path().join("plugins").join("mutagen");
        let agents = mutagen_root.join("agents");
        fs::create_dir_all(&agents).expect("agents dir");
        fs::write(
            agents.join("Probe.md"),
            "---\nname: Probe\n---\n\n# Probe persona body\n",
        )
        .expect("persona write");
        (dir, mutagen_root)
    }

    #[test]
    fn launch_agent_closes_stdin_by_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        unsafe {
            env::remove_var("MUTAGEN_AGENT_LAUNCHER_KEEP_STDIN");
        }

        let (dir, mutagen_root) = make_persona_workspace();
        let launcher = write_launcher_script(
            dir.path(),
            "#!/usr/bin/env bash\n\
             if read -r -t 0 line; then\n\
                printf 'STDIN_HAD_DATA: %s\\n' \"$line\"\n\
             else\n\
                printf 'STDIN_NULL\\n'\n\
             fi\n",
        );
        let stdout_capture = dir.path().join("capture.txt");

        unsafe {
            env::set_var("MUTAGEN_AGENT_LAUNCHER", &launcher);
        }
        let result = launch_agent(
            dir.path(),
            &mutagen_root,
            HostKind::Codex,
            "Probe",
            "probe task",
            &stdout_capture,
        );
        unsafe {
            env::remove_var("MUTAGEN_AGENT_LAUNCHER");
        }

        let status = result.expect("launcher should run");
        assert!(status.success(), "launcher exit: {status:?}");
        let captured = fs::read_to_string(&stdout_capture).expect("capture read");
        assert!(
            captured.contains("STDIN_NULL"),
            "stdin should be closed by default — captured: {captured:?}"
        );
    }

    #[test]
    fn launch_agent_preserves_stdin_when_opt_in_is_set() {
        let _guard = ENV_LOCK.lock().unwrap();

        let (dir, mutagen_root) = make_persona_workspace();
        // With stdin preserved the test process's own stdin is inherited.
        // We can't easily feed it anything from a #[test] body, so the probe
        // just records that the FD was *not* /dev/null — i.e. `read -t 0`
        // would still timeout, but the launcher can tell by checking whether
        // stdin is a regular file pointing at /dev/null. Simplest reliable
        // probe: `[[ -e /proc/self/fd/0 ]]` + readlink comparison.
        let launcher = write_launcher_script(
            dir.path(),
            "#!/usr/bin/env bash\n\
             target=$(readlink /proc/self/fd/0 2>/dev/null || true)\n\
             if [[ \"$target\" == /dev/null* ]]; then\n\
                printf 'STDIN_IS_DEVNULL\\n'\n\
             else\n\
                printf 'STDIN_IS_INHERITED: %s\\n' \"$target\"\n\
             fi\n",
        );
        let stdout_capture = dir.path().join("capture.txt");

        unsafe {
            env::set_var("MUTAGEN_AGENT_LAUNCHER", &launcher);
            env::set_var("MUTAGEN_AGENT_LAUNCHER_KEEP_STDIN", "1");
        }
        let result = launch_agent(
            dir.path(),
            &mutagen_root,
            HostKind::Codex,
            "Probe",
            "probe task",
            &stdout_capture,
        );
        unsafe {
            env::remove_var("MUTAGEN_AGENT_LAUNCHER");
            env::remove_var("MUTAGEN_AGENT_LAUNCHER_KEEP_STDIN");
        }

        let status = result.expect("launcher should run");
        assert!(status.success(), "launcher exit: {status:?}");
        let captured = fs::read_to_string(&stdout_capture).expect("capture read");
        assert!(
            !captured.contains("STDIN_IS_DEVNULL"),
            "opt-in should preserve the inherited stdin — captured: {captured:?}"
        );
    }
}

#[cfg(test)]
mod structural_micro_correction_tests {
    use super::*;

    #[test]
    fn detects_pure_traces_to_drift_findings() {
        let payload = json!({
            "findings": [
                {"check": "traces_to_drift", "severity": "fail", "detail": "cited ID FR-13 does not appear in author output"},
                {"check": "traces_to_drift", "severity": "fail", "detail": "cited ID DSD-622 does not appear in author output"},
            ]
        });
        assert!(findings_are_only_traces_to_drift(&payload));
    }

    #[test]
    fn rejects_mixed_findings() {
        let payload = json!({
            "findings": [
                {"check": "traces_to_drift", "severity": "fail", "detail": "cited ID FR-13 does not appear"},
                {"check": "required_section", "severity": "fail", "detail": "missing heading"},
            ]
        });
        assert!(!findings_are_only_traces_to_drift(&payload));
    }

    #[test]
    fn rejects_empty_findings() {
        let payload = json!({"findings": []});
        assert!(!findings_are_only_traces_to_drift(&payload));
    }

    #[test]
    fn writes_qa_report_with_one_fix_per_finding() {
        let dir = tempfile::tempdir().expect("tempdir");
        let payload = json!({
            "findings": [
                {"check": "traces_to_drift", "severity": "fail", "detail": "cited ID FR-13 does not appear in author output"},
                {"check": "traces_to_drift", "severity": "fail", "detail": "cited ID NFR-4 does not appear in author output"},
            ]
        });

        let report = write_structural_citations_qa_report(dir.path(), "L2-Workflow-002", &payload)
            .expect("report should write");

        let body = fs::read_to_string(&report.qa_report_path).expect("qa report should read");
        assert!(body.contains("Structural Citation Drift"));
        assert!(body.contains("Suggested Fix 1"));
        assert!(body.contains("Suggested Fix 2"));
        assert!(body.contains("FR-13"));
        assert!(body.contains("NFR-4"));

        let latest = fs::read_to_string(&report.latest_qa_report_path)
            .expect("latest qa report should read");
        assert_eq!(
            latest, body,
            "latest mirror should equal the slice-scoped report"
        );
    }
}
