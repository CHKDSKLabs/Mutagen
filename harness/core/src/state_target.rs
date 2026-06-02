use anyhow::{Result, bail};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const PROJECT_STATE_FILE: &str = "project_state.md";
pub const INFRASTRUCTURE_STATE_FILE: &str = "infrastructure_state.md";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StateTarget {
    pub context_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_section: Option<String>,
}

impl StateTarget {
    pub fn parse(context_to_update: &str) -> Result<Self> {
        let raw = context_to_update.trim();
        if raw.is_empty() {
            bail!("context target is empty");
        }

        if looks_like_parenthetical_section(raw) {
            bail!(
                "context target `{raw}` combines a file and section label as a path; use `project_state.md § <section>` or `infrastructure_state.md § <section>`"
            );
        }

        let (context_file, context_section) = if let Some((file, section)) = raw.split_once('§') {
            if section.contains('§') {
                bail!("context target `{raw}` contains multiple section separators");
            }

            let section = section.trim();
            if section.is_empty() {
                bail!("context target `{raw}` has an empty section label");
            }

            (file.trim(), Some(section.to_string()))
        } else {
            (raw, None)
        };

        let context_file = canonical_context_file(context_file)?;

        Ok(Self {
            context_file: context_file.to_string(),
            context_section,
        })
    }

    pub fn display(&self) -> String {
        match &self.context_section {
            Some(section) => format!("{} § {section}", self.context_file),
            None => self.context_file.clone(),
        }
    }

    pub fn workspace_path(&self, workspace_root: &Path) -> PathBuf {
        workspace_root.join(&self.context_file)
    }

    pub fn allowed_write_glob(&self) -> String {
        self.context_file.clone()
    }
}

fn canonical_context_file(value: &str) -> Result<&'static str> {
    let normalized = value.trim().replace('\\', "/");
    if normalized.contains('/') || normalized.contains("..") {
        bail!("context target `{value}` must be a repository-root state document");
    }

    match normalized.as_str() {
        PROJECT_STATE_FILE => Ok(PROJECT_STATE_FILE),
        INFRASTRUCTURE_STATE_FILE => Ok(INFRASTRUCTURE_STATE_FILE),
        _ => bail!(
            "context target `{value}` is not a canonical state document; expected `project_state.md` or `infrastructure_state.md`"
        ),
    }
}

fn looks_like_parenthetical_section(value: &str) -> bool {
    let value = value.trim();
    value.ends_with(')')
        && (value.starts_with(&format!("{PROJECT_STATE_FILE} ("))
            || value.starts_with(&format!("{INFRASTRUCTURE_STATE_FILE} (")))
}

#[cfg(test)]
mod tests {
    use super::StateTarget;

    #[test]
    fn parses_canonical_file_target() {
        let target = StateTarget::parse("project_state.md").expect("target should parse");

        assert_eq!(target.context_file, "project_state.md");
        assert_eq!(target.context_section, None);
    }

    #[test]
    fn parses_section_anchor_target() {
        let target =
            StateTarget::parse("infrastructure_state.md § CI").expect("target should parse");

        assert_eq!(target.context_file, "infrastructure_state.md");
        assert_eq!(target.context_section.as_deref(), Some("CI"));
        assert_eq!(target.display(), "infrastructure_state.md § CI");
    }

    #[test]
    fn rejects_parenthetical_section_path() {
        let error = StateTarget::parse("project_state.md (RBAC section)")
            .expect_err("parenthetical pseudo path should fail")
            .to_string();

        assert!(error.contains("combines a file and section label as a path"));
    }
}
