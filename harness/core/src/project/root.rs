//! Project Root value object — DDD §3.2.
//!
//! Constructor canonicalises via `std::fs::canonicalize` and refuses
//! relative paths outright (ISC-008 / INV-P1).

use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot {
    canonical: PathBuf,
}

#[derive(Debug)]
pub enum ProjectRootError {
    Relative,
    Canonicalize(std::io::Error),
}

impl fmt::Display for ProjectRootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Relative => f.write_str("project root must be an absolute path"),
            Self::Canonicalize(e) => write!(f, "canonicalize failed: {e}"),
        }
    }
}

impl std::error::Error for ProjectRootError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Canonicalize(e) => Some(e),
            _ => None,
        }
    }
}

impl ProjectRoot {
    /// Build a ProjectRoot. Rejects relatives (INV-P1) and resolves symlinks
    /// plus `.` / `..` segments so two callers using different spellings of
    /// the same directory cannot register twice (INV-P3, ISC-008).
    pub fn new(path: impl AsRef<Path>) -> Result<Self, ProjectRootError> {
        let raw = path.as_ref();
        if !raw.is_absolute() {
            return Err(ProjectRootError::Relative);
        }
        let canonical = std::fs::canonicalize(raw).map_err(ProjectRootError::Canonicalize)?;
        Ok(Self { canonical })
    }

    /// Wrap an already-canonical path. Used when re-hydrating from disk
    /// where we trust the registry's prior canonicalisation. The caller
    /// promises this path is the output of `canonicalize`.
    pub(crate) fn from_stored(canonical: PathBuf) -> Self {
        Self { canonical }
    }

    pub fn as_path(&self) -> &Path {
        &self.canonical
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.canonical
    }

    /// Equality per DDD §3.2: case-insensitive on Windows, byte-exact elsewhere.
    pub fn matches(&self, other: &Self) -> bool {
        path_eq(&self.canonical, &other.canonical)
    }
}

#[cfg(windows)]
fn path_eq(a: &Path, b: &Path) -> bool {
    // Windows filesystem semantics are case-insensitive for the paths we care
    // about; lowercasing the OS string is the cheap, correct comparison here.
    let a = a.as_os_str().to_string_lossy().to_lowercase();
    let b = b.as_os_str().to_string_lossy().to_lowercase();
    a == b
}

#[cfg(not(windows))]
fn path_eq(a: &Path, b: &Path) -> bool {
    a == b
}
