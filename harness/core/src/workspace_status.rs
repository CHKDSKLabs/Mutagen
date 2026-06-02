use serde::Serialize;
use std::path::Path;
use std::process::Command;

use crate::policy::path_matches_any_glob;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct WorkspaceDirtySummary {
    pub checked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
    pub modified_count: usize,
    pub untracked_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
}

pub fn summarize_dirty_paths(workspace_root: &Path, globs: &[String]) -> WorkspaceDirtySummary {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .arg("status")
        .arg("--porcelain=v1")
        .arg("-uall")
        .output();

    let Ok(output) = output else {
        return skipped("git status was unavailable");
    };

    if !output.status.success() {
        return skipped("workspace is not a git repository");
    }

    let status = String::from_utf8_lossy(&output.stdout);
    let mut summary = WorkspaceDirtySummary {
        checked: true,
        ..WorkspaceDirtySummary::default()
    };

    for line in status.lines() {
        let Some((kind, path)) = parse_porcelain_line(line) else {
            continue;
        };

        if !globs.is_empty() && !path_matches_any_glob(globs, &path).unwrap_or(false) {
            continue;
        }

        match kind {
            DirtyPathKind::Untracked => summary.untracked_count += 1,
            DirtyPathKind::Modified => summary.modified_count += 1,
        }

        if summary.paths.len() < 50 {
            summary.paths.push(path);
        }
    }

    summary
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirtyPathKind {
    Modified,
    Untracked,
}

fn parse_porcelain_line(line: &str) -> Option<(DirtyPathKind, String)> {
    if line.len() < 4 {
        return None;
    }

    let status = &line[..2];
    let path = line[3..].trim().trim_matches('"').replace('\\', "/");
    if path.is_empty() {
        return None;
    }

    if status == "??" {
        Some((DirtyPathKind::Untracked, path))
    } else {
        Some((DirtyPathKind::Modified, path))
    }
}

fn skipped(reason: &str) -> WorkspaceDirtySummary {
    WorkspaceDirtySummary {
        checked: false,
        skipped_reason: Some(reason.to_string()),
        ..WorkspaceDirtySummary::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{DirtyPathKind, parse_porcelain_line};

    #[test]
    fn parses_untracked_porcelain_line() {
        let parsed =
            parse_porcelain_line("?? .mutagen/state/active-slice.json").expect("line should parse");

        assert_eq!(parsed.0, DirtyPathKind::Untracked);
        assert_eq!(parsed.1, ".mutagen/state/active-slice.json");
    }

    #[test]
    fn parses_modified_porcelain_line() {
        let parsed = parse_porcelain_line(" M slices/queue.json").expect("line should parse");

        assert_eq!(parsed.0, DirtyPathKind::Modified);
        assert_eq!(parsed.1, "slices/queue.json");
    }
}
