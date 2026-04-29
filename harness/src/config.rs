use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineMode {
    #[default]
    Full,
    Lightweight,
}

impl fmt::Display for PipelineMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => write!(f, "full"),
            Self::Lightweight => write!(f, "lightweight"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowConfig {
    #[serde(default)]
    pub pipeline_mode: PipelineMode,
    #[serde(default = "default_max_parallel_slices")]
    pub max_parallel_slices: u32,
    #[serde(default)]
    pub review: ReviewConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewConfig {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_max_micro_corrections")]
    pub max_micro_corrections: u32,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            max_micro_corrections: default_max_micro_corrections(),
        }
    }
}

const fn default_max_retries() -> u32 {
    2
}

const fn default_max_micro_corrections() -> u32 {
    1
}

const fn default_max_parallel_slices() -> u32 {
    1
}

impl WorkflowConfig {
    pub fn normalized_max_parallel_slices(&self) -> u32 {
        self.max_parallel_slices.max(1)
    }
}

pub fn load_workflow_config_file(path: &Path) -> Result<WorkflowConfig> {
    if !path.exists() {
        return Ok(WorkflowConfig::default());
    }

    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read workflow config at {}", display_path(path)))?;

    serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse workflow config at {}", display_path(path)))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
