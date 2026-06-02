//! File-based advisory Project Lock.
//!
//! Lives at `<project_root>/.mutagen/state/project.lock`, with the
//! holder identity written to a sibling file `project.lock.holder`.
//! The split exists because Windows `LockFileEx` is mandatory and blocks
//! foreign reads of any locked byte — keeping identity in its own file
//! lets a peeking process read holder metadata without contesting the
//! OS lock. Backs ISC-004 (single writer per project) and ISC-005
//! (per-command, not per-session).

use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use sysinfo::{Pid, System};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub const LOCK_RELATIVE_PATH: &str = ".mutagen/state/project.lock";
pub const HOLDER_RELATIVE_PATH: &str = ".mutagen/state/project.lock.holder";

/// EX_NOPERM in sysexits.h — DDD §3.2 / ISC-004 documents 78 as the
/// "another writer holds the lock" exit code.
pub const EXIT_LOCK_HELD: i32 = 78;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LockHolder {
    Cli { pid: u32 },
    Service { session_id: String },
}

impl fmt::Display for LockHolder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display is human-friendly only — the canonical on-disk
        // serialization is JSON via LockRecord, not this string.
        match self {
            LockHolder::Cli { pid } => write!(f, "cli:{pid}"),
            LockHolder::Service { session_id } => write!(f, "service:{session_id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockRecord {
    pub holder: LockHolder,
    pub acquired_at: String,
}

#[derive(Debug)]
pub enum AcquireError {
    /// Held by a live process. Caller bails with EXIT_LOCK_HELD.
    Held(LockRecord),
    /// Held by a process we couldn't identify (no holder file, or the
    /// file was unparseable). Treat as live for safety.
    HeldAnonymous,
    /// IO blew up before we could decide. Surface to the operator.
    Io(anyhow::Error),
}

impl fmt::Display for AcquireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AcquireError::Held(rec) => write!(
                f,
                "project lock already held by {} since {}",
                rec.holder, rec.acquired_at
            ),
            AcquireError::HeldAnonymous => f.write_str("project lock held by unknown process"),
            AcquireError::Io(e) => write!(f, "project lock io error: {e:#}"),
        }
    }
}

impl std::error::Error for AcquireError {}

impl From<anyhow::Error> for AcquireError {
    fn from(e: anyhow::Error) -> Self {
        AcquireError::Io(e)
    }
}

pub fn lock_path(project_root: &Path) -> PathBuf {
    project_root.join(LOCK_RELATIVE_PATH)
}

pub fn holder_path(project_root: &Path) -> PathBuf {
    project_root.join(HOLDER_RELATIVE_PATH)
}

/// Best-effort holder probe — read the holder file without contending
/// for the OS lock. Returns `None` if the file is missing or empty.
pub fn read_record(project_root: &Path) -> Option<LockRecord> {
    let raw = fs::read_to_string(holder_path(project_root)).ok()?;
    parse_record(&raw)
}

fn parse_record(raw: &str) -> Option<LockRecord> {
    // Holder file is a single JSON line. Tiger Claw caught the old
    // "<holder> <ts>" wire format eating session_ids that contained
    // spaces or newlines (TC-1, TC-2) — JSON escapes both losslessly,
    // so any String the public API accepts now round-trips faithfully.
    let line = raw.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    serde_json::from_str::<LockRecord>(line).ok()
}

/// Acquire the project lock for `holder`.
///
/// Symmetric exclusion (POL-P1): same primitive backs CLI and service,
/// so whichever writer started first wins; the other gets `Held` with
/// the recorded identity.
///
/// Stale holders (INV-P5): if the recorded `Cli` PID is not running,
/// the lock is broken and re-claimed. `Service` holders are presumed
/// alive — heartbeat plumbing arrives in a later slice.
pub fn acquire(project_root: &Path, holder: LockHolder) -> Result<LockGuard, AcquireError> {
    let path = lock_path(project_root);
    let hpath = holder_path(project_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create lock parent dir {}", parent.display()))?;
    }

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .with_context(|| format!("open lock file {}", path.display()))?;

    if file.try_lock_exclusive().is_err() {
        // Couldn't grab the OS-level lock — somebody else holds it.
        // Read the sibling identity file (no contention) to decide
        // whether that holder is alive.
        return match read_record(project_root) {
            Some(rec) if !holder_alive(&rec.holder) => break_and_acquire(&path, &hpath, holder),
            Some(rec) => Err(AcquireError::Held(rec)),
            None => Err(AcquireError::HeldAnonymous),
        };
    }

    write_identity(&hpath, &holder)?;
    Ok(LockGuard {
        file: Some(file),
        path,
        holder_path: hpath,
        holder,
    })
}

fn break_and_acquire(
    lock_path: &Path,
    holder_path: &Path,
    holder: LockHolder,
) -> Result<LockGuard, AcquireError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .with_context(|| format!("re-open lock for break {}", lock_path.display()))?;
    if file.try_lock_exclusive().is_err() {
        // Someone else got there first.
        return match read_record(lock_path.parent().unwrap_or(Path::new("."))) {
            Some(rec) => Err(AcquireError::Held(rec)),
            None => Err(AcquireError::HeldAnonymous),
        };
    }
    write_identity(holder_path, &holder)?;
    Ok(LockGuard {
        file: Some(file),
        path: lock_path.to_path_buf(),
        holder_path: holder_path.to_path_buf(),
        holder,
    })
}

fn write_identity(holder_path: &Path, holder: &LockHolder) -> Result<(), AcquireError> {
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("format acquired_at timestamp")?;
    let record = LockRecord {
        holder: holder.clone(),
        acquired_at: now,
    };
    let mut line = serde_json::to_string(&record).context("serialize holder record")?;
    line.push('\n');

    // Atomic-ish replace via tmp + rename — rename is atomic on the
    // same filesystem on every OS we target, which is what we need so
    // a peeking process never sees a half-written line.
    let tmp = holder_path.with_extension("holder.tmp");
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .with_context(|| format!("open holder tmp {}", tmp.display()))?;
        std::io::Write::write_all(&mut f, line.as_bytes())
            .with_context(|| format!("write holder identity {}", tmp.display()))?;
        f.sync_all().context("fsync holder file")?;
    }
    fs::rename(&tmp, holder_path)
        .with_context(|| format!("rename holder tmp into {}", holder_path.display()))?;
    Ok(())
}

fn holder_alive(holder: &LockHolder) -> bool {
    match holder {
        LockHolder::Cli { pid } => pid_alive(*pid),
        // Service holders presume-alive; heartbeat-driven liveness
        // is a later-slice concern (Krang's service dispatch lands it).
        LockHolder::Service { .. } => true,
    }
}

fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let mut sys = System::new();
    sys.refresh_processes(
        sysinfo::ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
    );
    sys.process(Pid::from_u32(pid)).is_some()
}

/// RAII guard. Drop releases best-effort; explicit `release()` surfaces
/// IO errors. A process crash without unwind leaves the file behind —
/// exactly the case `pid_alive` was built to recover.
pub struct LockGuard {
    file: Option<File>,
    path: PathBuf,
    holder_path: PathBuf,
    holder: LockHolder,
}

impl LockGuard {
    pub fn holder(&self) -> &LockHolder {
        &self.holder
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn holder_path(&self) -> &Path {
        &self.holder_path
    }

    /// Verify the recorded identity still names *us*. Mutation helpers
    /// take `&LockGuard` and call this as a witness check at the writer
    /// boundary — that's the type-system enforcement ISC-004 cites.
    pub fn assert_owned(&self) -> Result<()> {
        let raw = fs::read_to_string(&self.holder_path).with_context(|| {
            format!(
                "project lock ownership drift: holder file unreadable at {}",
                self.holder_path.display()
            )
        })?;
        // Unparseable residue counts as drift, not as a generic IO/parse
        // error — somebody else rewrote the holder file or it's been
        // truncated. Both mean we no longer hold attestable ownership.
        let Some(rec) = parse_record(&raw) else {
            anyhow::bail!(
                "project lock ownership drift: holder file at {} is unparseable, expected {}",
                self.holder_path.display(),
                self.holder,
            );
        };
        if rec.holder != self.holder {
            anyhow::bail!(
                "project lock ownership drift: expected {}, found {} (acquired_at={})",
                self.holder,
                rec.holder,
                rec.acquired_at
            );
        }
        Ok(())
    }

    /// Explicit release. Drop performs the same work without bubbling
    /// IO errors; this exists for callers that want to surface them.
    pub fn release(mut self) -> Result<()> {
        self.release_inner()
    }

    fn release_inner(&mut self) -> Result<()> {
        if let Some(file) = self.file.take() {
            // Wipe identity before unlocking so a stale reader can't
            // attribute the file to us after release.
            let _ = fs::remove_file(&self.holder_path);
            FileExt::unlock(&file).context("unlock project lock")?;
        }
        Ok(())
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.release_inner();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(holder: LockHolder) -> LockRecord {
        let rec = LockRecord {
            holder,
            acquired_at: "2026-05-08T12:00:00Z".into(),
        };
        let line = serde_json::to_string(&rec).unwrap();
        // Verify single-line invariant — multi-line JSON would defeat
        // the read-first-line strategy parse_record relies on.
        assert!(
            !line.contains('\n'),
            "serialized record must be one line: {line}"
        );
        let parsed = parse_record(&format!("{line}\n")).expect("parse");
        assert_eq!(parsed, rec);
        parsed
    }

    #[test]
    fn json_record_round_trip_cli() {
        round_trip(LockHolder::Cli { pid: 4242 });
    }

    #[test]
    fn json_record_round_trip_service_plain() {
        round_trip(LockHolder::Service {
            session_id: "abc-123".into(),
        });
    }

    // TC-1 regression: a session_id with a space used to collide with the
    // "<holder> <ts>" wire format and silently decode to a different
    // identity. JSON encoding sidesteps the delimiter game entirely.
    #[test]
    fn json_record_round_trip_service_with_space() {
        round_trip(LockHolder::Service {
            session_id: "tab one".into(),
        });
    }

    // TC-2 regression: a session_id containing a newline used to truncate
    // because parse_record only read the first line. serde_json escapes
    // \n as \\n, so the on-disk representation stays single-line.
    #[test]
    fn json_record_round_trip_service_with_newline() {
        round_trip(LockHolder::Service {
            session_id: "line1\nline2".into(),
        });
    }

    #[test]
    fn parse_record_rejects_blank() {
        assert!(parse_record("").is_none());
        assert!(parse_record("\n").is_none());
        assert!(parse_record("   \n").is_none());
    }

    #[test]
    fn parse_record_rejects_garbage() {
        assert!(parse_record("not json at all\n").is_none());
        assert!(parse_record("cli:99 2026-05-08T12:00:00Z\n").is_none());
    }
}
