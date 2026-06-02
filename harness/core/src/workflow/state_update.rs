//! Append-only State Update log writer.
//!
//! ISC-006 / INV-W4: this is the only sanctioned writer for the
//! State Update JSONL. We open with platform-correct append semantics
//! (`O_APPEND` on Unix via `OpenOptions::append(true)`; the same call
//! lowers to `FILE_APPEND_DATA | SYNCHRONIZE` on Windows). No seek+write
//! API is exposed, by design — every caller goes through `append_record`.
//!
//! ISC-007 / INV-W5: the record type carries a mandatory `origin: Origin`.
//! Empty origins can't exist because `Origin` constructors reject them
//! (see `crate::workflow::origin`). New records also carry a
//! `schema_version` field so replay can distinguish pre-0.4.0 records
//! (no `schema_version`, tolerated for backward compat per MD-4) from
//! post-0.4.0 records (must carry origin or fail-closed).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::project::lock::LockGuard;
use crate::workflow::origin::Origin;

pub const LOG_RELATIVE_PATH: &str = ".mutagen/state/log.jsonl";
pub const OFFSET_RELATIVE_PATH: &str = ".mutagen/state/log.offset";

/// Schema version stamped on every record this writer produces.
/// Bump in lockstep with a successor ADR; absence signals pre-0.4.0.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateUpdate {
    pub schema_version: u32,
    pub slice_id: String,
    pub event: String,
    pub at: String,
    pub origin: Origin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

impl StateUpdate {
    pub fn new(
        slice_id: impl Into<String>,
        event: impl Into<String>,
        at: impl Into<String>,
        origin: Origin,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            slice_id: slice_id.into(),
            event: event.into(),
            at: at.into(),
            origin,
            body: None,
        }
    }
}

pub fn log_path(project_root: &Path) -> PathBuf {
    project_root.join(LOG_RELATIVE_PATH)
}

pub fn offset_path(project_root: &Path) -> PathBuf {
    project_root.join(OFFSET_RELATIVE_PATH)
}

/// Append one record to the State Update log.
///
/// The `_lock` parameter is a witness — only a process that currently
/// holds the Project Lock may call this. We call `assert_owned()` to
/// catch ownership drift (somebody rewrote the holder file from under
/// us) before touching the log.
///
/// Returns the new file size in bytes, which is also persisted to
/// `state/log.offset` so replay can detect truncation.
pub fn append_record(lock: &LockGuard, project_root: &Path, record: &StateUpdate) -> Result<u64> {
    lock.assert_owned()
        .context("state update writer requires owned project lock")?;

    let path = log_path(project_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create log dir {}", parent.display()))?;
    }

    let mut line = serde_json::to_string(record).context("serialize state update record")?;
    debug_assert!(
        !line.contains('\n'),
        "serialized record must be single-line JSON"
    );
    line.push('\n');

    // append(true) is the platform-correct append. On Unix this lowers
    // to O_APPEND so concurrent writers don't clobber each other; on
    // Windows the std lib opens with FILE_APPEND_DATA which has the
    // same atomic-append semantics for sub-PIPE_BUF-sized writes.
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open state update log {}", path.display()))?;
    f.write_all(line.as_bytes())
        .with_context(|| format!("append to state update log {}", path.display()))?;
    f.sync_all()
        .context("fsync state update log after append")?;

    let size = fs::metadata(&path)
        .with_context(|| format!("stat state update log {}", path.display()))?
        .len();
    write_offset(project_root, size)?;
    Ok(size)
}

fn write_offset(project_root: &Path, size: u64) -> Result<()> {
    let path = offset_path(project_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create offset parent dir {}", parent.display()))?;
    }
    fs::write(&path, size.to_string())
        .with_context(|| format!("persist log offset to {}", path.display()))?;
    Ok(())
}

pub fn read_offset(project_root: &Path) -> Result<Option<u64>> {
    let path = offset_path(project_root);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("read offset file {}", path.display()))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed: u64 = trimmed
        .parse()
        .with_context(|| format!("parse offset file {} as u64", path.display()))?;
    Ok(Some(parsed))
}
