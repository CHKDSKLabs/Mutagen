use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::config::PipelineMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliceQueue {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub generated_by: String,
    #[serde(default)]
    pub pipeline_mode: PipelineMode,
    #[serde(default)]
    pub planning_advisories: Vec<PlanningAdvisory>,
    #[serde(default)]
    pub slices: Vec<Slice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slice {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub status: SliceStatus,
    #[serde(default)]
    pub author_agent: String,
    #[serde(default)]
    pub layer: u32,
    #[serde(default)]
    pub bounded_context: String,
    #[serde(default)]
    pub target_loc: u32,
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub context_to_update: String,
    #[serde(default)]
    pub implementation_details: Vec<String>,
    #[serde(default)]
    pub review_required: bool,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub micro_corrections_used: u32,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub adjacent_scope_allowed: Vec<String>,
    #[serde(default)]
    pub write_set: Vec<String>,
    #[serde(default)]
    pub traces_to: TraceSet,
    #[serde(default)]
    pub verification_steps: VerificationSteps,
    #[serde(default)]
    pub human_check_needed: HumanCheckNeeded,
    #[serde(default, skip_serializing_if = "SliceVerdicts::is_empty")]
    pub verdicts: SliceVerdicts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalation_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum SliceStatus {
    #[default]
    Pending,
    InProgress,
    BlockedRetry,
    Completed,
    Escalated,
    Refused,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SliceVerdicts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub karai_structural: Option<KaraiStructuralVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bishop: Option<BishopVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiger_claw: Option<TigerClawVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub micro_correction: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub micro_corrections_used: Option<u32>,
}

impl SliceVerdicts {
    pub fn is_empty(&self) -> bool {
        self.karai_structural.is_none()
            && self.bishop.is_none()
            && self.tiger_claw.is_none()
            && self.micro_correction.is_none()
            && self.micro_corrections_used.is_none()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum KaraiStructuralVerdict {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum BishopVerdict {
    Clean,
    Advisory,
    Block,
    Skip,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum TigerClawVerdict {
    Clean,
    Gap,
    Defect,
    Skip,
}

impl SliceStatus {
    pub fn is_ready_candidate(self) -> bool {
        matches!(self, Self::Pending | Self::BlockedRetry)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceSet {
    #[serde(default)]
    pub prd: Vec<String>,
    #[serde(default)]
    pub adr: Vec<String>,
    #[serde(default)]
    pub ddd: Vec<String>,
    #[serde(default)]
    pub isc: Vec<String>,
    #[serde(default)]
    pub dsd: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerificationSteps {
    #[serde(default)]
    pub acceptance: String,
    #[serde(default)]
    pub isc_detection: String,
    #[serde(default)]
    pub dsd_conformance: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HumanCheckNeeded {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanningAdvisory {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub severity: AdvisorySeverity,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub decision: String,
    #[serde(default)]
    pub user_response_required: bool,
    #[serde(default)]
    pub references: Vec<String>,
    #[serde(default)]
    pub affects_slices: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdvisorySeverity {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlockedSlice {
    pub id: String,
    pub unmet_dependencies: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum NextSliceSelection {
    Ready { index: usize },
    QueueClear,
    Stalled { blocked: Vec<BlockedSlice> },
}

impl SliceQueue {
    pub fn select_next_ready_slice(&self) -> NextSliceSelection {
        let mut blocked = Vec::new();

        for (index, slice) in self.slices.iter().enumerate() {
            if !slice.status.is_ready_candidate() {
                continue;
            }

            let unmet_dependencies = self.unmet_dependencies_for(slice);
            if unmet_dependencies.is_empty() {
                return NextSliceSelection::Ready { index };
            }

            blocked.push(BlockedSlice {
                id: slice.id.clone(),
                unmet_dependencies,
            });
        }

        if blocked.is_empty() {
            NextSliceSelection::QueueClear
        } else {
            NextSliceSelection::Stalled { blocked }
        }
    }

    pub fn claim_slice(&mut self, index: usize) {
        if let Some(slice) = self.slices.get_mut(index) {
            slice.status = SliceStatus::InProgress;
        }
    }

    pub fn unmet_dependencies_for(&self, slice: &Slice) -> Vec<String> {
        let completed: HashSet<&str> = self
            .slices
            .iter()
            .filter(|candidate| candidate.status == SliceStatus::Completed)
            .map(|candidate| candidate.id.as_str())
            .collect();

        slice
            .depends_on
            .iter()
            .filter(|dependency| !completed.contains(dependency.as_str()))
            .cloned()
            .collect()
    }

    pub fn slice_mut(&mut self, slice_id: &str) -> Option<&mut Slice> {
        self.slices.iter_mut().find(|slice| slice.id == slice_id)
    }
}

const fn default_version() -> u32 {
    1
}
