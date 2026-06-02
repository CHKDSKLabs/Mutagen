//! Project Registry — DDD §3.2, ISC-008.
//!
//! Persists at `<service_data>/projects.toml`. Whole-file atomic write
//! (write-to-tmp + rename) so a crash mid-save never leaves a torn registry.

use crate::project::root::{ProjectRoot, ProjectRootError};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

/// Soft lifecycle states. `archived` is a tombstone — the entry is kept so
/// historical ids stay resolvable, but the root is free for re-registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Registered,
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectEntry {
    pub project_id: String,
    pub name: String,
    pub root: PathBuf,
    pub status: ProjectStatus,
    pub created_at: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default, rename = "project")]
    projects: Vec<ProjectEntry>,
}

#[derive(Debug)]
pub struct ProjectRegistry {
    path: PathBuf,
    file: RegistryFile,
}

#[derive(Debug)]
pub enum RegisterError {
    Root(ProjectRootError),
    DuplicateRoot { existing_id: String },
    Persist(anyhow::Error),
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root(e) => write!(f, "{e}"),
            Self::DuplicateRoot { existing_id } => {
                write!(f, "root already registered as project {existing_id}")
            }
            Self::Persist(e) => write!(f, "persist registry: {e}"),
        }
    }
}

impl std::error::Error for RegisterError {}

#[derive(Debug)]
pub enum LookupError {
    NotFound,
}

#[derive(Debug)]
pub enum ArchiveError {
    NotFound,
    Persist(anyhow::Error),
}

impl ProjectRegistry {
    /// Open the registry at `path`. Missing file is fine — it just means
    /// "no projects yet" and the first save materialises the file.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let file = match fs::read_to_string(&path) {
            Ok(raw) => toml::from_str::<RegistryFile>(&raw)
                .with_context(|| format!("parse registry at {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => RegistryFile::default(),
            Err(e) => {
                return Err(
                    anyhow::Error::from(e).context(format!("read registry at {}", path.display()))
                );
            }
        };
        Ok(Self { path, file })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> &[ProjectEntry] {
        &self.file.projects
    }

    pub fn lookup(&self, project_id: &str) -> Result<&ProjectEntry, LookupError> {
        self.file
            .projects
            .iter()
            .find(|p| p.project_id == project_id)
            .ok_or(LookupError::NotFound)
    }

    /// Register a new project. Canonicalises the root (ISC-008), refuses
    /// any non-archived collision on the same canonical root (INV-P3), and
    /// stamps a server-generated UUIDv7 (DSD-623).
    pub fn register(
        &mut self,
        name: impl Into<String>,
        root_path: impl AsRef<Path>,
    ) -> Result<ProjectEntry, RegisterError> {
        let root = ProjectRoot::new(root_path).map_err(RegisterError::Root)?;

        for existing in &self.file.projects {
            if existing.status == ProjectStatus::Archived {
                continue;
            }
            let existing_root = ProjectRoot::from_stored(existing.root.clone());
            if existing_root.matches(&root) {
                return Err(RegisterError::DuplicateRoot {
                    existing_id: existing.project_id.clone(),
                });
            }
        }

        let created_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"));

        let entry = ProjectEntry {
            project_id: Uuid::now_v7().to_string(),
            name: name.into(),
            root: root.into_path_buf(),
            status: ProjectStatus::Registered,
            created_at,
        };
        self.file.projects.push(entry.clone());
        self.save().map_err(RegisterError::Persist)?;
        Ok(entry)
    }

    pub fn archive(&mut self, project_id: &str) -> Result<(), ArchiveError> {
        let entry = self
            .file
            .projects
            .iter_mut()
            .find(|p| p.project_id == project_id)
            .ok_or(ArchiveError::NotFound)?;
        entry.status = ProjectStatus::Archived;
        self.save().map_err(ArchiveError::Persist)
    }

    /// Atomic whole-file write: serialize, write to `<path>.tmp`, fsync, rename.
    /// Torn-write protection — if the process dies between create and rename,
    /// the original file is untouched.
    fn save(&self) -> Result<()> {
        let serialized =
            toml::to_string_pretty(&self.file).context("serialize project registry")?;

        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("create registry parent {}", parent.display()))?;
        }

        let tmp = tmp_path_for(&self.path);
        {
            let mut f =
                fs::File::create(&tmp).with_context(|| format!("create tmp {}", tmp.display()))?;
            f.write_all(serialized.as_bytes())
                .with_context(|| format!("write tmp {}", tmp.display()))?;
            f.sync_all().ok();
        }
        fs::rename(&tmp, &self.path)
            .with_context(|| format!("rename {} -> {}", tmp.display(), self.path.display()))?;
        Ok(())
    }
}

fn tmp_path_for(target: &Path) -> PathBuf {
    let mut name = target
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_else(|| std::ffi::OsString::from("projects.toml"));
    name.push(".tmp");
    match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(name),
        _ => PathBuf::from(name),
    }
}
