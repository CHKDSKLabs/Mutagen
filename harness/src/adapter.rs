use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::config::WorkflowConfig;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    Stub,
    Codex,
    Claude,
}

impl Default for HostKind {
    fn default() -> Self {
        Self::Stub
    }
}

pub trait HostAdapter {
    fn kind(&self) -> HostKind;
    fn capabilities(&self) -> HostCapabilities;

    fn execution_profile(&self, workflow_config: &WorkflowConfig) -> HostExecutionProfile {
        HostExecutionProfile::from_capabilities(self.kind(), self.capabilities(), workflow_config)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostCapabilities {
    pub can_enforce_pre_write: bool,
    pub can_isolate_worktree: bool,
    pub can_stream_tool_events: bool,
    pub can_restrict_tools_per_stage: bool,
    pub can_interrupt_running_stage: bool,
}

impl HostCapabilities {
    pub fn degraded_features(self) -> Vec<String> {
        let mut degraded = Vec::new();

        if !self.can_enforce_pre_write {
            degraded.push("pre_write_scope_enforcement".to_string());
        }
        if !self.can_isolate_worktree {
            degraded.push("worktree_isolation".to_string());
        }
        if !self.can_stream_tool_events {
            degraded.push("tool_event_streaming".to_string());
        }
        if !self.can_restrict_tools_per_stage {
            degraded.push("per_stage_tool_restriction".to_string());
        }
        if !self.can_interrupt_running_stage {
            degraded.push("stage_interrupts".to_string());
        }

        degraded
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScopeEnforcementMode {
    Hard,
    Advisory,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeIsolationMode {
    IsolatedWorktree,
    SharedWorkspace,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParallelDispatchMode {
    SerialOnly,
    BoundedCohort,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolEventCollectionMode {
    Streaming,
    Polling,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageToolRestrictionMode {
    Enforced,
    Advisory,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageInterruptMode {
    Supported,
    ManualOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostDowngrade {
    pub feature: String,
    pub requested: String,
    pub effective: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostExecutionProfile {
    pub host: HostKind,
    pub capabilities: HostCapabilities,
    pub scope_enforcement: ScopeEnforcementMode,
    pub worktree_isolation: WorktreeIsolationMode,
    pub parallel_dispatch: ParallelDispatchMode,
    pub requested_max_parallel_slices: u32,
    pub effective_max_parallel_slices: u32,
    pub tool_event_collection: ToolEventCollectionMode,
    pub stage_tool_restriction: StageToolRestrictionMode,
    pub stage_interrupts: StageInterruptMode,
    pub degraded_features: Vec<String>,
    pub downgrades: Vec<HostDowngrade>,
}

impl HostExecutionProfile {
    fn from_capabilities(
        host: HostKind,
        capabilities: HostCapabilities,
        workflow_config: &WorkflowConfig,
    ) -> Self {
        let requested_max_parallel_slices = workflow_config.normalized_max_parallel_slices();
        let mut degraded_features = capabilities.degraded_features();
        let mut downgrades = Vec::new();

        let scope_enforcement = if capabilities.can_enforce_pre_write {
            ScopeEnforcementMode::Hard
        } else {
            downgrades.push(HostDowngrade {
                feature: "pre_write_scope_enforcement".to_string(),
                requested: "hard".to_string(),
                effective: "advisory".to_string(),
                reason: "host cannot block writes before tool execution".to_string(),
            });
            ScopeEnforcementMode::Advisory
        };

        let worktree_isolation = if capabilities.can_isolate_worktree {
            WorktreeIsolationMode::IsolatedWorktree
        } else {
            downgrades.push(HostDowngrade {
                feature: "worktree_isolation".to_string(),
                requested: "isolated_worktree".to_string(),
                effective: "shared_workspace".to_string(),
                reason: "host cannot provision isolated worktrees for sibling slices".to_string(),
            });
            WorktreeIsolationMode::SharedWorkspace
        };

        let parallel_dispatch = if requested_max_parallel_slices > 1
            && capabilities.can_isolate_worktree
        {
            ParallelDispatchMode::BoundedCohort
        } else {
            if requested_max_parallel_slices > 1 {
                degraded_features.push("parallel_dispatch".to_string());
                downgrades.push(HostDowngrade {
                    feature: "parallel_dispatch".to_string(),
                    requested: format!("{requested_max_parallel_slices}_sibling_slices"),
                    effective: "serial_only".to_string(),
                    reason: "bounded parallel dispatch requires isolated worktrees, which this host does not support".to_string(),
                });
            }
            ParallelDispatchMode::SerialOnly
        };

        let effective_max_parallel_slices = match parallel_dispatch {
            ParallelDispatchMode::SerialOnly => 1,
            ParallelDispatchMode::BoundedCohort => requested_max_parallel_slices,
        };

        let tool_event_collection = if capabilities.can_stream_tool_events {
            ToolEventCollectionMode::Streaming
        } else {
            downgrades.push(HostDowngrade {
                feature: "tool_event_streaming".to_string(),
                requested: "streaming".to_string(),
                effective: "polling".to_string(),
                reason: "host cannot stream tool events back to the harness".to_string(),
            });
            ToolEventCollectionMode::Polling
        };

        let stage_tool_restriction = if capabilities.can_restrict_tools_per_stage {
            StageToolRestrictionMode::Enforced
        } else {
            downgrades.push(HostDowngrade {
                feature: "per_stage_tool_restriction".to_string(),
                requested: "enforced".to_string(),
                effective: "advisory".to_string(),
                reason: "host cannot hard-restrict the tool surface per stage".to_string(),
            });
            StageToolRestrictionMode::Advisory
        };

        let stage_interrupts = if capabilities.can_interrupt_running_stage {
            StageInterruptMode::Supported
        } else {
            downgrades.push(HostDowngrade {
                feature: "stage_interrupts".to_string(),
                requested: "supported".to_string(),
                effective: "manual_only".to_string(),
                reason: "host cannot interrupt an in-flight stage on demand".to_string(),
            });
            StageInterruptMode::ManualOnly
        };

        degraded_features.sort();
        degraded_features.dedup();

        Self {
            host,
            capabilities,
            scope_enforcement,
            worktree_isolation,
            parallel_dispatch,
            requested_max_parallel_slices,
            effective_max_parallel_slices,
            tool_event_collection,
            stage_tool_restriction,
            stage_interrupts,
            degraded_features,
            downgrades,
        }
    }
}

pub fn adapter_for(host: HostKind) -> Box<dyn HostAdapter> {
    match host {
        HostKind::Stub => Box::new(StubHostAdapter),
        HostKind::Codex => Box::new(CodexHostAdapter),
        HostKind::Claude => Box::new(ClaudeHostAdapter),
    }
}

pub fn resolved_host_profile(
    host: HostKind,
    workflow_config: &WorkflowConfig,
) -> HostExecutionProfile {
    adapter_for(host).execution_profile(workflow_config)
}

#[derive(Debug, Default)]
pub struct StubHostAdapter;

impl HostAdapter for StubHostAdapter {
    fn kind(&self) -> HostKind {
        HostKind::Stub
    }

    fn capabilities(&self) -> HostCapabilities {
        HostCapabilities {
            can_enforce_pre_write: false,
            can_isolate_worktree: false,
            can_stream_tool_events: false,
            can_restrict_tools_per_stage: false,
            can_interrupt_running_stage: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct CodexHostAdapter;

impl HostAdapter for CodexHostAdapter {
    fn kind(&self) -> HostKind {
        HostKind::Codex
    }

    fn capabilities(&self) -> HostCapabilities {
        HostCapabilities {
            can_enforce_pre_write: false,
            can_isolate_worktree: false,
            can_stream_tool_events: false,
            can_restrict_tools_per_stage: true,
            can_interrupt_running_stage: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct ClaudeHostAdapter;

impl HostAdapter for ClaudeHostAdapter {
    fn kind(&self) -> HostKind {
        HostKind::Claude
    }

    fn capabilities(&self) -> HostCapabilities {
        HostCapabilities {
            can_enforce_pre_write: true,
            can_isolate_worktree: true,
            can_stream_tool_events: true,
            can_restrict_tools_per_stage: true,
            can_interrupt_running_stage: true,
        }
    }
}
