//! Crash-recovery replay for the State Update log.
//!
//! Two integrity checks per ISC-006:
//! 1. The log file MUST NOT have shrunk since the last persisted byte
//!    offset. Shrinkage means somebody truncated, edited, or replaced
//!    the file — fail-closed, prompt manual recovery.
//! 2. Every post-0.4.0 record (any record carrying `schema_version`)
//!    MUST carry a non-empty `origin`. Missing origin on a post-0.4.0
//!    record is corruption; fail-closed.
//!
//! Pre-0.4.0 records (no `schema_version`, no `origin`) are tolerated
//! per MD-4 — older installations have logs without these fields and
//! we don't want a version bump to make them un-replayable.

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::Path;

use crate::workflow::origin::Origin;
use crate::workflow::state_update::{log_path, read_offset};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ReplayedRecord {
    #[serde(default)]
    pub schema_version: Option<u32>,
    pub slice_id: String,
    pub event: String,
    pub at: String,
    #[serde(default)]
    pub origin: Option<Origin>,
    #[serde(default)]
    pub body: Option<Value>,
}

pub fn replay(project_root: &Path) -> Result<Vec<ReplayedRecord>> {
    let path = log_path(project_root);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let current_size = fs::metadata(&path)
        .with_context(|| format!("stat state update log {}", path.display()))?
        .len();
    if let Some(prev) = read_offset(project_root)?
        && current_size < prev
    {
        bail!(
            "state update log shrank from {prev} to {current_size} bytes at {} \
             — fail-closed startup per ISC-006; manual recovery required (inspect the log, \
             restore from backup, or run an explicit reset before continuing)",
            path.display()
        );
    }

    let raw = fs::read_to_string(&path)
        .with_context(|| format!("read state update log {}", path.display()))?;

    let mut out = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let record = parse_record(line, idx + 1)?;
        out.push(record);
    }
    Ok(out)
}

fn parse_record(line: &str, line_no: usize) -> Result<ReplayedRecord> {
    let record: ReplayedRecord = serde_json::from_str(line)
        .with_context(|| format!("parse state update log line {line_no}"))?;

    let is_post_0_4_0 = record.schema_version.is_some();
    if is_post_0_4_0 {
        let origin = record
            .origin
            .as_ref()
            .ok_or_else(|| anyhow!(
                "state update record at line {line_no} declares schema_version={} but is missing the mandatory `origin` field (ISC-007); fail-closed",
                record.schema_version.unwrap()
            ))?;
        if let Origin::Service { session_id } = origin
            && session_id.trim().is_empty()
        {
            bail!(
                "state update record at line {line_no} has an empty service session_id; fail-closed per ISC-007"
            );
        }
    }

    Ok(record)
}
