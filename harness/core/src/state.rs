use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::adapter::HostKind;
use crate::config::{PipelineMode, WorkflowConfig};
use crate::policy::{author_stage_write_globs, dedupe_globs};
use crate::queue::Slice;
use crate::queue_readiness::QueueReadinessSnapshot;
use crate::state_target::StateTarget;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Author,
    StructuralCheck,
    Review,
    StateRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveSliceState {
    pub slice_id: String,
    pub title: String,
    pub evidence_bundle_path: String,
    pub author_agent: String,
    pub active_agent: String,
    pub stage: Stage,
    pub pipeline_mode: PipelineMode,
    pub review_required: bool,
    pub layer: u32,
    pub bounded_context: String,
    pub context_to_update: String,
    #[serde(default)]
    pub context_file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_section: Option<String>,
    pub attempts: u32,
    pub max_retries: u32,
    pub micro_corrections_used: u32,
    pub max_micro_corrections: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_unix_ms: Option<u64>,
    pub allowed_write_globs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub amendments: Vec<ActiveScopeAmendment>,
    pub host: HostKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degraded_capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_readiness: Option<ActiveQueueReadiness>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveScopeAmendment {
    pub ts: String,
    pub added: Vec<String>,
    pub reason: String,
    pub justification_gap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveQueueReadiness {
    pub queue_path: String,
    pub queue_validation_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_contract_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_contract_hash_basis: Option<String>,
}

impl ActiveSliceState {
    pub fn from_slice(
        slice: &Slice,
        workflow_config: WorkflowConfig,
        host: HostKind,
        degraded_capabilities: Vec<String>,
        evidence_bundle_path: String,
    ) -> Result<Self> {
        let state_target = slice.state_target()?;

        Ok(Self {
            slice_id: slice.id.clone(),
            title: slice.title.clone(),
            evidence_bundle_path,
            author_agent: slice.author_agent.clone(),
            active_agent: slice.author_agent.clone(),
            stage: Stage::Author,
            pipeline_mode: workflow_config.pipeline_mode,
            review_required: slice.review_required,
            layer: slice.layer,
            bounded_context: slice.bounded_context.clone(),
            context_to_update: slice.context_to_update.clone(),
            context_file: state_target.context_file,
            context_section: state_target.context_section,
            attempts: slice.attempts,
            max_retries: workflow_config.review.max_retries,
            micro_corrections_used: slice.micro_corrections_used,
            max_micro_corrections: workflow_config.review.max_micro_corrections,
            started_at_unix_ms: None,
            allowed_write_globs: author_stage_write_globs(slice)?,
            amendments: Vec::new(),
            host,
            degraded_capabilities,
            queue_readiness: None,
        })
    }

    pub fn set_queue_readiness(&mut self, snapshot: QueueReadinessSnapshot) {
        self.queue_readiness = Some(ActiveQueueReadiness {
            queue_path: snapshot.queue_path,
            queue_validation_path: snapshot.queue_validation_path,
            queue_contract_hash: snapshot.queue_contract_hash,
            queue_contract_hash_basis: snapshot.queue_contract_hash_basis,
        });
    }

    pub fn set_author_stage(&mut self, active_agent: String, allowed_write_globs: Vec<String>) {
        self.active_agent = active_agent;
        self.stage = Stage::Author;
        self.allowed_write_globs = dedupe_globs(allowed_write_globs);
        self.amendments.clear();
    }

    pub fn set_structural_check_stage(&mut self) {
        self.active_agent = "Karai".to_string();
        self.stage = Stage::StructuralCheck;
        self.allowed_write_globs = vec![".mutagen/state/**".to_string()];
        self.amendments.clear();
    }

    pub fn set_review_stage(&mut self) {
        let mut globs = vec!["reviews/**".to_string(), "tests/qa/**".to_string()];
        if self.author_agent == "Tatsu" {
            globs.push("tests/qa/security/**".to_string());
        }
        globs.push(".mutagen/state/**".to_string());

        self.active_agent = "TigerClaw".to_string();
        self.stage = Stage::Review;
        self.allowed_write_globs = dedupe_globs(globs);
        self.amendments.clear();
    }

    pub fn state_target(&self) -> Result<StateTarget> {
        if self.context_file.trim().is_empty() {
            return StateTarget::parse(&self.context_to_update);
        }

        let raw = match self.context_section.as_deref() {
            Some(section) => format!("{} § {section}", self.context_file),
            None => self.context_file.clone(),
        };
        StateTarget::parse(&raw)
    }

    pub fn set_state_record_stage(&mut self) -> Result<()> {
        let target = self.state_target()?;

        self.active_agent = "Karai".to_string();
        self.stage = Stage::StateRecord;
        self.allowed_write_globs = vec![
            target.allowed_write_glob(),
            "slices/**".to_string(),
            ".mutagen/state/**".to_string(),
        ];
        self.amendments.clear();
        Ok(())
    }

    pub fn apply_amendment(
        &mut self,
        ts: String,
        added: Vec<String>,
        reason: String,
        justification_gap: bool,
    ) {
        let mut globs = self.allowed_write_globs.clone();
        globs.extend(added.clone());
        self.allowed_write_globs = dedupe_globs(globs);
        self.amendments.push(ActiveScopeAmendment {
            ts,
            added,
            reason,
            justification_gap,
        });
    }
}

pub fn load_active_slice(path: &Path) -> Result<ActiveSliceState> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read active slice at {}", display_path(path)))?;

    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse active slice at {}", display_path(path)))
}

pub fn write_active_slice(path: &Path, active_slice: &ActiveSliceState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(path)
            )
        })?;
    }

    let body = serde_json::to_string_pretty(active_slice)
        .context("failed to serialize active slice JSON")?;
    fs::write(path, format!("{body}\n"))
        .with_context(|| format!("failed to write {}", display_path(path)))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
