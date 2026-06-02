use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::queue::Slice;
use crate::state_target::StateTarget;
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
    if pre_fence_mentions(&section, slice_id)
        && section
            .lines()
            .any(|line| line.trim_start().starts_with("```"))
    {
        bail!(
            "State Update marker was found BEFORE the fenced block. Move the marker INSIDE the ``` fence -- the parser only reads fenced content once a fence is present.\n\n{}",
            state_update_format_help(slice_id)
        );
    }
    let body = extract_state_update_body(&section)?;

    let lines: Vec<&str> = body.lines().collect();
    let nonempty: Vec<(usize, String)> = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some((idx, trimmed.to_string()))
            }
        })
        .collect();

    let (first_idx, first) = nonempty.first().cloned().with_context(|| {
        format!(
            "State Update block for `{slice_id}` is empty.\n\n{}",
            state_update_format_help(slice_id)
        )
    })?;

    if is_diff_prefixed(&first) {
        bail!(
            "State Update block looks like a unified-diff fence. Drop the `+`/`-`/`@@` prefixes.\n\n{}",
            state_update_format_help(slice_id)
        );
    }

    // 2026-05-12 L4-Session-001: author led with `target: project_state.md § Sessions`
    // before the marker and the whole block bounced. Tolerate one yaml-ish `key: value`
    // prefix line; past that we go back to the strict "first non-blank line is the marker"
    // rule so two narrative lines can't smuggle a marker into position 3.
    let metadata_eats_first = !first.contains(slice_id) && is_metadata_pair_line(&first);
    let (marker_idx, marker) = if metadata_eats_first {
        match nonempty.get(1).cloned() {
            Some((idx, candidate)) => {
                if is_diff_prefixed(&candidate) {
                    bail!(
                        "State Update block looks like a unified-diff fence. Drop the `+`/`-`/`@@` prefixes.\n\n{}",
                        state_update_format_help(slice_id)
                    );
                }
                (idx, candidate)
            }
            None => (first_idx, first.clone()),
        }
    } else {
        (first_idx, first.clone())
    };

    if !marker.contains(slice_id) {
        let marker_after_narrative = lines
            .iter()
            .enumerate()
            .skip(marker_idx + 1)
            .any(|(_, line)| line.contains(slice_id));

        let leading_line_label = if marker_idx == first_idx {
            format!("first line was `{marker}`")
        } else {
            format!(
                "leading metadata line was `{first}`; next line was `{marker}` and did not carry the marker"
            )
        };

        let mut message = format!(
            "State Update block must contain a slice marker; it must start with a slice marker mentioning `{slice_id}` -- {leading_line_label}"
        );
        if pre_fence_mentions(&section, slice_id) {
            message.push_str(
                "\n\nHint: a line mentioning the slice id was found BEFORE the fenced block. \
                 Move it INSIDE the ``` fence -- the parser only reads fenced content \
                 once a fence is present.",
            );
        } else if marker_after_narrative {
            message.push_str(
                "\n\nHint: a slice marker was found later in the block. It must be the FIRST non-blank line.",
            );
        }
        message.push_str("\n\n");
        message.push_str(&state_update_format_help(slice_id));
        bail!("{}", message);
    }

    // Marker uniqueness: a second `### …` heading line that also names the
    // slice id is almost certainly a copy-paste accident. The persistence path
    // dedups on the marker string, so two markers would silently collapse —
    // refuse instead.
    let extra_marker = lines
        .iter()
        .enumerate()
        .skip(marker_idx + 1)
        .any(|(_, line)| {
            let trimmed = line.trim();
            trimmed.starts_with("###") && trimmed.contains(slice_id)
        });
    if extra_marker {
        bail!(
            "State Update block contains more than one slice marker for `{slice_id}`. Keep exactly one `### {slice_id} — <YYYY-MM-DD>` line."
        );
    }

    let body = if metadata_eats_first && marker_idx != first_idx {
        let mut retained: Vec<&str> = Vec::with_capacity(lines.len().saturating_sub(1));
        for (idx, line) in lines.iter().enumerate() {
            if idx != first_idx {
                retained.push(*line);
            }
        }
        retained
            .join("\n")
            .trim_start_matches('\n')
            .trim_end()
            .to_string()
    } else {
        body
    };

    Ok(ParsedStateUpdate { body, marker })
}

fn is_diff_prefixed(line: &str) -> bool {
    line.starts_with('+') || line.starts_with('-') || line.starts_with("@@")
}

fn is_metadata_pair_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    let mut key_end = first.len_utf8();
    for ch in chars {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            key_end += ch.len_utf8();
        } else {
            break;
        }
    }
    trimmed[key_end..].trim_start().starts_with(':')
}

fn state_update_format_help(slice_id: &str) -> String {
    format!(
        "Expected format -- the marker must be the FIRST non-blank line inside a markdown\n\
         fenced block under `## State Update`, on its own line, no `+`/`-`/`@@` diff prefix:\n\n\
         ## State Update\n\n\
         ```\n\
         ### {slice_id} — <YYYY-MM-DD>\n\n\
         (notes about what this slice changed)\n\
         ```"
    )
}

fn pre_fence_mentions(section: &str, slice_id: &str) -> bool {
    section
        .lines()
        .take_while(|line| !line.trim_start().starts_with("```"))
        .any(|line| line.contains(slice_id))
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

pub fn apply_state_update_to_target(
    workspace_root: &Path,
    target: &StateTarget,
    update: &ParsedStateUpdate,
) -> Result<(String, bool)> {
    let context_path = target.workspace_path(workspace_root);
    let already_present = if let Some(section) = target.context_section.as_deref() {
        apply_state_update_block_to_section(&context_path, section, update)?
    } else {
        apply_state_update_block(&context_path, update)?
    };

    Ok((display_path(&context_path), already_present))
}

fn apply_state_update_block_to_section(
    context_path: &Path,
    section: &str,
    update: &ParsedStateUpdate,
) -> Result<bool> {
    if let Some(parent) = context_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for {}",
                display_path(context_path)
            )
        })?;
    }

    let context = if context_path.exists() {
        fs::read_to_string(context_path)
            .with_context(|| format!("failed to read {}", display_path(context_path)))?
    } else {
        String::new()
    };

    if text_contains_marker(&context, &update.marker) {
        return Ok(true);
    }

    let next = upsert_section_update(&context, section, update);
    fs::write(context_path, next)
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
    let target = slice.state_target()?;
    let (context_path, already_present) =
        apply_state_update_to_target(&workspace_root, &target, &update)?;

    Ok(ApplyStateUpdateResult {
        slice_id: slice.id.clone(),
        context_path,
        author_output_path: display_path(&author_output_path),
        marker: update.marker,
        already_present,
    })
}

pub fn context_path_for_slice(workspace_root: &Path, slice: &Slice) -> Result<PathBuf> {
    Ok(slice.state_target()?.workspace_path(workspace_root))
}

fn extract_state_update_section(author_output: &str) -> Result<String> {
    let lines: Vec<&str> = author_output.lines().collect();
    let mut start = None;
    let mut level = 0usize;

    for (index, line) in lines.iter().enumerate() {
        if let Some((candidate_level, title)) = heading(line)
            && title.to_lowercase().starts_with("state update")
        {
            start = Some(index + 1);
            level = candidate_level;
            break;
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

        if !in_fence
            && let Some((candidate_level, _)) = heading(line)
            && candidate_level <= level
            && body.iter().any(|entry| !entry.trim().is_empty())
        {
            break;
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

fn text_contains_marker(body: &str, marker: &str) -> bool {
    let marker = marker.trim();
    if marker.is_empty() {
        return false;
    }

    body.lines().map(str::trim).any(|line| line == marker)
}

fn upsert_section_update(context: &str, section: &str, update: &ParsedStateUpdate) -> String {
    let lines = context.lines().collect::<Vec<_>>();
    let Some((start, level)) = find_section_heading(&lines, section) else {
        return append_missing_section(context, section, update);
    };

    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find_map(|(index, line)| {
            heading(line)
                .filter(|(candidate_level, _)| *candidate_level <= level)
                .map(|_| index)
        })
        .unwrap_or(lines.len());

    let mut next = String::new();
    next.push_str(&lines[..end].join("\n"));
    if !next.ends_with('\n') {
        next.push('\n');
    }
    if !next.trim_end().ends_with(&update.marker) {
        next.push('\n');
        next.push_str(update.body.trim());
        next.push('\n');
    }
    if end < lines.len() {
        next.push('\n');
        next.push_str(&lines[end..].join("\n"));
        next.push('\n');
    }
    next
}

fn append_missing_section(context: &str, section: &str, update: &ParsedStateUpdate) -> String {
    let mut next = String::new();
    if !context.trim().is_empty() {
        next.push_str(context.trim_end());
        next.push_str("\n\n");
    }
    next.push_str(&format!("## {section}\n\n"));
    next.push_str(update.body.trim());
    next.push('\n');
    next
}

fn find_section_heading(lines: &[&str], section: &str) -> Option<(usize, usize)> {
    let expected = normalize_heading_title(section);
    lines.iter().enumerate().find_map(|(index, line)| {
        let (level, title) = heading(line)?;
        (normalize_heading_title(title) == expected).then_some((index, level))
    })
}

fn normalize_heading_title(title: &str) -> String {
    title
        .trim()
        .trim_end_matches('#')
        .trim()
        .to_ascii_lowercase()
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
