//! Project wire types — DDD §3.2, ISC-010.
//!
//! Domain `ProjectEntry` from `mutagen-core` never crosses the route boundary.
//! Handlers translate to/from these DTOs explicitly so a future field rename in
//! the registry can't silently rewrite the published OpenAPI spec.

use mutagen_core::project::registry::{ProjectEntry, ProjectStatus};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectDto {
    /// Stable UUIDv7 (DSD-623) assigned at registration. Sort-friendly.
    pub project_id: String,
    /// Operator-supplied display name. Free-form, non-empty.
    pub name: String,
    /// Canonical absolute path of the project root (ISC-008).
    pub root: String,
    /// Lifecycle: `registered` | `active` | `archived`.
    pub status: String,
    /// RFC 3339 / ISO 8601 UTC timestamp (DSD-622).
    pub created_at: String,
}

impl ProjectDto {
    pub fn from_entry(entry: &ProjectEntry) -> Self {
        Self {
            project_id: entry.project_id.clone(),
            name: entry.name.clone(),
            root: entry.root.display().to_string(),
            status: status_str(entry.status).to_string(),
            created_at: entry.created_at.clone(),
        }
    }
}

fn status_str(s: ProjectStatus) -> &'static str {
    match s {
        ProjectStatus::Registered => "registered",
        ProjectStatus::Active => "active",
        ProjectStatus::Archived => "archived",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RegisterProjectRequest {
    /// Absolute filesystem path to the project workspace.
    pub root: String,
    /// Display name for the project. Trimmed; must be non-empty.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ProjectListDto {
    pub items: Vec<ProjectDto>,
}
