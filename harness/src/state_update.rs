use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::queue::Slice;
use crate::validation::load_queue_file;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ParsedStateUpdate {
    pub body: String,
    pub marker: String,
}

#[derive(Debug, Clone)]
pub struct ApplyStateUpdateOptions {
    pub workspace_root: PathBuf,
    pub queue_path: PathBuf,
    pub slice_id: String,
    pub author_output_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ApplyStateUpdateResult {
    pub slice_id: String,
    pub context_path: String,
    pub author_output_path: String,
    pub marker: String,
    pub already_present: bool,
}

pub fn parse_state_update(author_output: &str, slice_id: &str) -> Result<ParsedStateUpdate> {
    if slice_id.trim().is_empty() {
        bail!("missing `slice_id`");
    }

    let section = extract_state_update_section(author_output)?;
    let body = extract_state_update_body(&section)?;
    let marker = first_nonempty_line(&body)
        .with_context(|| format!("State Update block for `{slice_id}` is empty"))?;

    if !marker.contains(slice_id) {
        bail!("State Update block must start with a slice marker containing `{slice_id}`");
    }

    Ok(ParsedStateUpdate { body, marker })
}

pub fn apply_state_update_block(context_path: &Path, update: &ParsedStateUpdate) -> Result<bool> {
    if let Some(parent) = context_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(context_path)
            )
        })?;
    }

    let mut context = if context_path.exists() {
        fs::read_to_string(context_path)
            .with_context(|| format!("failed to read {}", display_path(context_path)))?
    } else {
        String::new()
    };

    if text_contains_marker(&context, &update.marker) {
        return Ok(true);
    }

    if !context.is_empty() && !context.ends_with('\n') {
        context.push('\n');
    }

    if !context.trim().is_empty() {
        context.push('\n');
    }

    context.push_str(update.body.trim());
    if !context.ends_with('\n') {
        context.push('\n');
    }

    fs::write(context_path, context)
        .with_context(|| format!("failed to write {}", display_path(context_path)))?;

    Ok(false)
}

pub fn context_contains_state_update(context_path: &Path, marker: &str) -> Result<bool> {
    if !context_path.exists() {
        return Ok(false);
    }

    let context = fs::read_to_string(context_path)
        .with_context(|| format!("failed to read {}", display_path(context_path)))?;
    Ok(text_contains_marker(&context, marker))
}

pub fn apply_state_update_for_slice(
    options: ApplyStateUpdateOptions,
) -> Result<ApplyStateUpdateResult> {
    if options.slice_id.trim().is_empty() {
        bail!("missing `slice_id`");
    }

    let workspace_root = resolve_workspace_root(&options.workspace_root)?;
    let queue_path = resolve_workspace_path(&workspace_root, &options.queue_path);
    let queue = load_queue_file(&queue_path)?;
    let slice = queue
        .slices
        .iter()
        .find(|slice| slice.id == options.slice_id)
        .with_context(|| format!("slice `{}` not found", options.slice_id))?;

    let author_output_path = options.author_output_path.unwrap_or_else(|| {
        workspace_root
            .join(".mutagen/state/author-output")
            .join(format!("{}.md", safe_file_name(&slice.id)))
    });
    let author_output_path = resolve_workspace_path(&workspace_root, &author_output_path);
    let author_output = fs::read_to_string(&author_output_path).with_context(|| {
        format!(
            "failed to read author output at {}",
            display_path(&author_output_path)
        )
    })?;
    let update = parse_state_update(&author_output, &slice.id)?;
    let context_path = resolve_workspace_path(&workspace_root, Path::new(&slice.context_to_update));
    let already_present = apply_state_update_block(&context_path, &update)?;

    Ok(ApplyStateUpdateResult {
        slice_id: slice.id.clone(),
        context_path: display_path(&context_path),
        author_output_path: display_path(&author_output_path),
        marker: update.marker,
        already_present,
    })
}

pub fn context_path_for_slice(workspace_root: &Path, slice: &Slice) -> PathBuf {
    resolve_workspace_path(workspace_root, Path::new(&slice.context_to_update))
}

fn extract_state_update_section(author_output: &str) -> Result<String> {
    let lines: Vec<&str> = author_output.lines().collect();
    let mut start = None;
    let mut level = 0usize;

    for (index, line) in lines.iter().enumerate() {
        if let Some((candidate_level, title)) = heading(line) {
            if title.to_lowercase().starts_with("state update") {
                start = Some(index + 1);
                level = candidate_level;
                break;
            }
        }
    }

    let start = start.context("author output is missing a `State Update` section")?;
    let mut body = Vec::new();
    let mut in_fence = false;

    for line in lines.iter().skip(start) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            body.push(*line);
            continue;
        }

        if !in_fence {
            if let Some((candidate_level, _)) = heading(line) {
                if candidate_level <= level && body.iter().any(|entry| !entry.trim().is_empty()) {
                    break;
                }
            }
        }

        body.push(*line);
    }

    let section = body.join("\n").trim().to_string();
    if section.is_empty() {
        bail!("State Update section is empty");
    }

    Ok(section)
}

fn extract_state_update_body(section: &str) -> Result<String> {
    let lines: Vec<&str> = section.lines().collect();
    let opening_fence = lines
        .iter()
        .position(|line| line.trim_start().starts_with("```"));

    let Some(opening_fence) = opening_fence else {
        return Ok(section.trim().to_string());
    };

    let mut body = Vec::new();
    for line in lines.iter().skip(opening_fence + 1) {
        if line.trim_start().starts_with("```") {
            let fenced = body.join("\n").trim().to_string();
            if fenced.is_empty() {
                bail!("State Update fenced block is empty");
            }
            return Ok(fenced);
        }
        body.push(*line);
    }

    bail!("State Update fenced block is missing a closing fence")
}

fn first_nonempty_line(body: &str) -> Option<String> {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn text_contains_marker(body: &str, marker: &str) -> bool {
    let marker = marker.trim();
    if marker.is_empty() {
        return false;
    }

    body.lines().map(str::trim).any(|line| line == marker)
}

fn heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if level == 0 {
        return None;
    }

    let title = trimmed[level..].trim_start();
    if title.is_empty() {
        return None;
    }

    Some((level, title))
}

fn resolve_workspace_root(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        bail!("missing `workspace_root`");
    }

    if path.exists() {
        fs::canonicalize(path)
            .with_context(|| format!("failed to resolve workspace root {}", display_path(path)))
    } else if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("failed to read current working directory")?
            .join(path))
    }
}

fn resolve_workspace_path(workspace_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn safe_file_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
