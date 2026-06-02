use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::policy::{
    dedupe_globs, default_author_write_set, first_matching_glob, path_matches_any_glob,
};
use crate::queue::Slice;
use crate::state::{ActiveSliceState, Stage, load_active_slice, write_active_slice};
use crate::validation::load_queue_file;

const GLOBAL_DENY_GLOBS: &[&str] = &[
    ".git",
    ".git/**",
    "templates/**",
    "guides/**",
    "docs/PRD*",
    "docs/PRD/**",
    "docs/ADR*",
    "docs/ADR/**",
    "docs/DDD*",
    "docs/DDD/**",
    "docs/ISC*",
    "docs/ISC/**",
    "docs/DSD*",
    "docs/DSD/**",
    "PRD*.md",
    "ADR*.md",
    "DDD*.md",
    "ISC*.md",
    "DSD*.md",
    "design/**",
    ".env",
    ".env.*",
    "secrets/**",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "Cargo.lock",
];

const GENERIC_PATH_TOKENS: &[&str] = &[
    "src",
    "app",
    "api",
    "components",
    "pages",
    "tests",
    "docs",
    "infrastructure",
    "terraform",
    "migrations",
    "schema",
    "db",
    "prisma",
    "public",
    "styles",
    "middleware",
    "policies",
    "observability",
    "alerts",
    "dashboards",
    "runbooks",
    "reviews",
    "qa",
    "state",
    "mutagen",
];

#[derive(Debug, Clone)]
pub struct AmendScopeOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub active_state_path: PathBuf,
    pub amendments_log_path: PathBuf,
    pub requested_globs: Vec<String>,
    pub mutation_kind: MutationKind,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum MutationKind {
    Create,
    Modify,
    Delete,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AmendmentDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DenyClass {
    Global,
    StageFidelity,
    AgentDomain,
}

#[derive(Debug, Clone, Serialize)]
pub struct AmendScopeResult {
    pub decision: AmendmentDecision,
    pub active_state_path: String,
    pub amendments_log_path: String,
    pub slice_id: String,
    pub title: String,
    pub stage: Stage,
    pub active_agent: String,
    pub mutation_kind: MutationKind,
    pub requested_globs: Vec<String>,
    pub reason: String,
    pub rationale: String,
    pub suggested_next_step: String,
    pub justification_gap: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub added_globs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_write_globs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<DenyClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_rule: Option<String>,
}

#[derive(Debug, Serialize)]
struct AmendmentLogEntry {
    ts: String,
    slice: String,
    stage: Stage,
    agent: String,
    mutation_kind: MutationKind,
    requested: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    added: Vec<String>,
    reason: String,
    decision: AmendmentDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    class: Option<DenyClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    matched_rule: Option<String>,
    justification_gap: bool,
    suggested_next_step: String,
}

pub fn amend_scope(options: AmendScopeOptions) -> Result<AmendScopeResult> {
    let requested_globs = normalize_requested_globs(&options.requested_globs);
    if requested_globs.is_empty() {
        bail!("missing at least one `requested_glob`");
    }

    if options.reason.trim().is_empty() {
        bail!("missing `reason`");
    }

    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let queue_path = resolve_workspace_path(&workspace_root, &options.queue_path);
    let active_state_path = resolve_workspace_path(&workspace_root, &options.active_state_path);
    let amendments_log_path = resolve_workspace_path(&workspace_root, &options.amendments_log_path);

    let queue = load_queue_file(&queue_path)?;
    let mut active_state = load_active_slice(&active_state_path)?;
    let slice = queue
        .slices
        .iter()
        .find(|slice| slice.id == active_state.slice_id)
        .with_context(|| format!("slice `{}` not found in queue", active_state.slice_id))?;

    let justification_gap = has_justification_gap(slice, &requested_globs);
    let ts = now_iso_utc()?;

    if let Some(denial) = evaluate_denial(slice, &active_state, &requested_globs)? {
        let result = AmendScopeResult {
            decision: AmendmentDecision::Deny,
            active_state_path: display_path(&active_state_path),
            amendments_log_path: display_path(&amendments_log_path),
            slice_id: active_state.slice_id.clone(),
            title: active_state.title.clone(),
            stage: active_state.stage,
            active_agent: active_state.active_agent.clone(),
            mutation_kind: options.mutation_kind,
            requested_globs: requested_globs.clone(),
            reason: options.reason.trim().to_string(),
            rationale: denial.rationale.clone(),
            suggested_next_step: denial.suggested_next_step.clone(),
            justification_gap,
            added_globs: Vec::new(),
            allowed_write_globs: active_state.allowed_write_globs.clone(),
            class: Some(denial.class),
            matched_rule: denial.matched_rule.clone(),
        };

        append_amendment_log(
            &amendments_log_path,
            AmendmentLogEntry {
                ts,
                slice: active_state.slice_id,
                stage: active_state.stage,
                agent: active_state.active_agent,
                mutation_kind: options.mutation_kind,
                requested: requested_globs,
                added: Vec::new(),
                reason: options.reason.trim().to_string(),
                decision: AmendmentDecision::Deny,
                class: Some(denial.class),
                matched_rule: denial.matched_rule,
                justification_gap,
                suggested_next_step: denial.suggested_next_step,
            },
        )?;

        return Ok(result);
    }

    let added_globs: Vec<String> = requested_globs
        .iter()
        .filter(|glob| !active_state.allowed_write_globs.contains(*glob))
        .cloned()
        .collect();

    if !added_globs.is_empty() {
        active_state.apply_amendment(
            ts.clone(),
            added_globs.clone(),
            options.reason.trim().to_string(),
            justification_gap,
        );
        write_active_slice(&active_state_path, &active_state)?;
    }

    append_amendment_log(
        &amendments_log_path,
        AmendmentLogEntry {
            ts,
            slice: active_state.slice_id.clone(),
            stage: active_state.stage,
            agent: active_state.active_agent.clone(),
            mutation_kind: options.mutation_kind,
            requested: requested_globs.clone(),
            added: added_globs.clone(),
            reason: options.reason.trim().to_string(),
            decision: AmendmentDecision::Allow,
            class: None,
            matched_rule: None,
            justification_gap,
            suggested_next_step:
                "amendment is live for the current stage; re-run the blocked action.".to_string(),
        },
    )?;

    Ok(AmendScopeResult {
        decision: AmendmentDecision::Allow,
        active_state_path: display_path(&active_state_path),
        amendments_log_path: display_path(&amendments_log_path),
        slice_id: active_state.slice_id,
        title: active_state.title,
        stage: active_state.stage,
        active_agent: active_state.active_agent,
        mutation_kind: options.mutation_kind,
        requested_globs,
        reason: options.reason.trim().to_string(),
        rationale: if added_globs.is_empty() {
            "requested globs were already present in the active manifest.".to_string()
        } else {
            "requested globs fit the current stage, the active agent's domain, and the deny rules."
                .to_string()
        },
        suggested_next_step: "amendment is live for the current stage; re-run the blocked action."
            .to_string(),
        justification_gap,
        added_globs,
        allowed_write_globs: active_state.allowed_write_globs,
        class: None,
        matched_rule: None,
    })
}

#[derive(Debug)]
struct Denial {
    class: DenyClass,
    matched_rule: Option<String>,
    rationale: String,
    suggested_next_step: String,
}

fn evaluate_denial(
    slice: &Slice,
    active_state: &ActiveSliceState,
    requested_globs: &[String],
) -> Result<Option<Denial>> {
    let global_deny_globs: Vec<String> = GLOBAL_DENY_GLOBS
        .iter()
        .map(|glob| glob.to_string())
        .collect();
    let stage_policy_globs = stage_policy_globs(slice, active_state)?;
    let agent_domain_globs = agent_domain_globs(slice, active_state)?;

    for requested_glob in requested_globs {
        if let Some(matched_rule) = first_matching_glob(&global_deny_globs, requested_glob)? {
            return Ok(Some(Denial {
                class: DenyClass::Global,
                matched_rule: Some(matched_rule),
                rationale: format!(
                    "`{requested_glob}` matches the global denylist and cannot be widened into the active manifest."
                ),
                suggested_next_step:
                    "re-slice via `/mutagen:slice` or reassign the work to the owning agent."
                        .to_string(),
            }));
        }

        if !path_matches_any_glob(&stage_policy_globs, requested_glob)? {
            return Ok(Some(Denial {
                class: DenyClass::StageFidelity,
                matched_rule: None,
                rationale: format!(
                    "`{requested_glob}` does not belong to the `{}` stage manifest.",
                    stage_name(active_state.stage)
                ),
                suggested_next_step: suggest_stage_next_step(slice, active_state, requested_glob)?,
            }));
        }

        if !path_matches_any_glob(&agent_domain_globs, requested_glob)? {
            let owning_agent = infer_owning_agent(slice, requested_glob)?
                .unwrap_or_else(|| "the owning agent".to_string());

            return Ok(Some(Denial {
                class: DenyClass::AgentDomain,
                matched_rule: None,
                rationale: format!(
                    "`{requested_glob}` falls outside {}'s domain.",
                    active_state.active_agent
                ),
                suggested_next_step: format!(
                    "re-slice so {owning_agent} owns this work, or escalate to the human."
                ),
            }));
        }
    }

    Ok(None)
}

fn stage_policy_globs(slice: &Slice, active_state: &ActiveSliceState) -> Result<Vec<String>> {
    match active_state.stage {
        Stage::Author => author_stage_globs_for_agent(slice, &active_state.active_agent),
        Stage::StructuralCheck => Ok(vec![".mutagen/state/**".to_string()]),
        Stage::Review => {
            let mut globs = vec![
                "reviews/**".to_string(),
                "tests/qa/**".to_string(),
                ".mutagen/state/**".to_string(),
            ];
            if slice.author_agent == "Tatsu" {
                globs.push("tests/qa/security/**".to_string());
            }
            Ok(dedupe_globs(globs))
        }
        Stage::StateRecord => Ok(vec![
            slice.state_target()?.allowed_write_glob(),
            "slices/**".to_string(),
            ".mutagen/state/**".to_string(),
        ]),
    }
}

fn agent_domain_globs(slice: &Slice, active_state: &ActiveSliceState) -> Result<Vec<String>> {
    match active_state.stage {
        Stage::Author => author_stage_globs_for_agent(slice, &active_state.active_agent),
        Stage::StructuralCheck => Ok(vec![".mutagen/state/**".to_string()]),
        Stage::Review => Ok(vec![
            "reviews/**".to_string(),
            "tests/qa/**".to_string(),
            "tests/qa/security/**".to_string(),
            ".mutagen/state/**".to_string(),
        ]),
        Stage::StateRecord => Ok(vec![
            slice.state_target()?.allowed_write_glob(),
            "slices/**".to_string(),
            ".mutagen/state/**".to_string(),
        ]),
    }
}

fn author_stage_globs_for_agent(slice: &Slice, active_agent: &str) -> Result<Vec<String>> {
    let mut globs = if active_agent == slice.author_agent && !slice.write_set.is_empty() {
        slice.write_set.clone()
    } else {
        default_author_write_set(active_agent)?
    };

    globs.push(".mutagen/state/**".to_string());

    Ok(dedupe_globs(globs))
}

fn suggest_stage_next_step(
    slice: &Slice,
    active_state: &ActiveSliceState,
    requested_glob: &str,
) -> Result<String> {
    let review_state = ActiveSliceState {
        stage: Stage::Review,
        ..active_state.clone()
    };
    if path_matches_any_glob(&stage_policy_globs(slice, &review_state)?, requested_glob)? {
        return Ok("wait for stage `review`, where that path is already permitted.".to_string());
    }

    let state_record_state = ActiveSliceState {
        stage: Stage::StateRecord,
        ..active_state.clone()
    };
    if path_matches_any_glob(
        &stage_policy_globs(slice, &state_record_state)?,
        requested_glob,
    )? {
        return Ok(
            "wait for stage `state_record`, where that path is already permitted.".to_string(),
        );
    }

    Ok("re-slice via `/mutagen:slice` or adjust the request to fit the current stage.".to_string())
}

fn infer_owning_agent(slice: &Slice, requested_glob: &str) -> Result<Option<String>> {
    for agent in [
        "Bebop",
        "Chaplin",
        "Metalhead",
        "Splinter",
        "Tatsu",
        "Krang",
    ] {
        let globs = if agent == slice.author_agent && !slice.write_set.is_empty() {
            slice.write_set.clone()
        } else {
            default_author_write_set(agent)?
        };

        if path_matches_any_glob(&globs, requested_glob)? {
            return Ok(Some(agent.to_string()));
        }
    }

    Ok(None)
}

fn has_justification_gap(slice: &Slice, requested_globs: &[String]) -> bool {
    let haystack = format!(
        "{} {} {} {} {}",
        slice.title,
        slice.objective,
        slice.bounded_context,
        slice.implementation_details.join(" "),
        slice.traces_to.ddd.join(" ")
    )
    .to_lowercase();

    requested_globs.iter().any(|glob| {
        let normalized = glob.replace('\\', "/").to_lowercase();
        if !slice.bounded_context.trim().is_empty()
            && normalized.contains(&slice.bounded_context.to_lowercase())
        {
            return false;
        }

        let tokens: Vec<String> = normalized
            .split('/')
            .flat_map(|segment| segment.split(['.', '-', '_']))
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .filter(|token| !token.contains('*'))
            .filter(|token| !GENERIC_PATH_TOKENS.contains(token))
            .map(str::to_string)
            .collect();

        if tokens.is_empty() {
            return true;
        }

        !tokens.iter().any(|token| haystack.contains(token))
    })
}

fn normalize_requested_globs(globs: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();

    for glob in globs {
        let normalized_glob = glob.trim().trim_matches('`').replace('\\', "/");
        if normalized_glob.is_empty() || normalized.contains(&normalized_glob) {
            continue;
        }
        normalized.push(normalized_glob);
    }

    normalized
}

fn append_amendment_log(path: &Path, entry: AmendmentLogEntry) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(path)
            )
        })?;
    }

    let line = serde_json::to_string(&entry).context("failed to serialize amendment log entry")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", display_path(path)))?;
    writeln!(file, "{line}")
        .with_context(|| format!("failed to append amendment log at {}", display_path(path)))
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

fn now_iso_utc() -> Result<String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("failed to format UTC timestamp")
}
