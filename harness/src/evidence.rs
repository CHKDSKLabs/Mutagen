use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::queue::Slice;

pub fn render_evidence_bundle(workspace_root: &Path, slice: &Slice) -> Result<String> {
    let prd_doc = load_named_doc_if_needed(workspace_root, "PRD", !slice.traces_to.prd.is_empty())?;
    let ddd_doc = load_named_doc_if_needed(workspace_root, "DDD", !slice.traces_to.ddd.is_empty())?;
    let isc_doc = load_named_doc_if_needed(workspace_root, "ISC", !slice.traces_to.isc.is_empty())?;
    let dsd_doc = load_named_doc_if_needed(workspace_root, "DSD", !slice.traces_to.dsd.is_empty())?;

    let prd_blocks = unique(&slice.traces_to.prd)
        .into_iter()
        .map(|citation| {
            let excerpt = extract_section_containing_marker(
                prd_doc.as_deref().unwrap_or_default(),
                &citation,
            )
            .ok_or_else(|| anyhow::anyhow!("failed to resolve PRD citation `{citation}`"))?;
            Ok(render_block(&citation, &excerpt))
        })
        .collect::<Result<Vec<_>>>()?;

    let adr_blocks = unique(&slice.traces_to.adr)
        .into_iter()
        .map(|citation| {
            let path = resolve_adr_path(workspace_root, &citation)?;
            let body = fs::read_to_string(&path)
                .with_context(|| format!("failed to read ADR file at {}", display_path(&path)))?;
            Ok(render_block(&citation, body.trim()))
        })
        .collect::<Result<Vec<_>>>()?;

    let ddd_blocks = unique(&slice.traces_to.ddd)
        .into_iter()
        .map(|citation| {
            let excerpt =
                extract_section_matching_heading(ddd_doc.as_deref().unwrap_or_default(), &citation)
                    .or_else(|| {
                        extract_section_containing_marker(
                            ddd_doc.as_deref().unwrap_or_default(),
                            &citation,
                        )
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!("failed to resolve DDD citation `{citation}`")
                    })?;
            Ok(render_block(&citation, &excerpt))
        })
        .collect::<Result<Vec<_>>>()?;

    let isc_blocks = unique(&slice.traces_to.isc)
        .into_iter()
        .map(|citation| {
            let excerpt = extract_section_containing_marker(
                isc_doc.as_deref().unwrap_or_default(),
                &citation,
            )
            .ok_or_else(|| anyhow::anyhow!("failed to resolve ISC citation `{citation}`"))?;
            Ok(render_block(&citation, &excerpt))
        })
        .collect::<Result<Vec<_>>>()?;

    let dsd_blocks = unique(&slice.traces_to.dsd)
        .into_iter()
        .map(|citation| {
            let excerpt = extract_section_containing_marker(
                dsd_doc.as_deref().unwrap_or_default(),
                &citation,
            )
            .ok_or_else(|| anyhow::anyhow!("failed to resolve DSD citation `{citation}`"))?;
            Ok(render_block(&citation, &excerpt))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok([
        format!("## Evidence Bundle for {}", slice.id),
        String::new(),
        render_section("PRD citations", prd_blocks),
        render_section("ADR(s)", adr_blocks),
        render_section("DDD citations", ddd_blocks),
        render_section("ISC citations", isc_blocks),
        render_section("DSD citations", dsd_blocks),
    ]
    .join("\n"))
}

pub fn write_evidence_bundle(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for evidence bundle {}",
                display_path(path)
            )
        })?;
    }

    fs::write(path, format!("{body}\n"))
        .with_context(|| format!("failed to write evidence bundle {}", display_path(path)))
}

fn load_named_doc_if_needed(
    workspace_root: &Path,
    name: &str,
    required: bool,
) -> Result<Option<String>> {
    if !required {
        return Ok(None);
    }

    let path = resolve_named_doc_path(workspace_root, name)?;
    let body = fs::read_to_string(&path).with_context(|| {
        format!(
            "failed to read {} document at {}",
            name,
            display_path(&path)
        )
    })?;

    Ok(Some(body))
}

fn resolve_named_doc_path(workspace_root: &Path, name: &str) -> Result<PathBuf> {
    let candidates = [
        workspace_root
            .join("docs")
            .join(name)
            .join(format!("{name}.md")),
        workspace_root.join("docs").join(format!("{name}.md")),
        workspace_root.join(format!("{name}.md")),
    ];

    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "failed to resolve {name} document under {}",
                display_path(workspace_root)
            )
        })
}

fn resolve_adr_path(workspace_root: &Path, citation: &str) -> Result<PathBuf> {
    let normalized_citation = normalize(citation);
    let candidates = collect_adr_candidates(workspace_root)?;

    candidates
        .into_iter()
        .find(|path| {
            let stem = path
                .file_stem()
                .map(|stem| normalize(&stem.to_string_lossy()))
                .unwrap_or_default();

            stem == normalized_citation || stem.contains(&normalized_citation)
        })
        .ok_or_else(|| anyhow::anyhow!("failed to resolve ADR citation `{citation}`"))
}

fn collect_adr_candidates(workspace_root: &Path) -> Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    collect_markdown_files(&workspace_root.join("docs").join("ADR"), &mut candidates)?;
    collect_markdown_files(&workspace_root.join("docs"), &mut candidates)?;
    collect_markdown_files(workspace_root, &mut candidates)?;

    candidates.retain(|path| {
        path.file_name()
            .map(|name| {
                name.to_string_lossy()
                    .to_ascii_uppercase()
                    .starts_with("ADR-")
            })
            .unwrap_or(false)
    });

    Ok(unique_paths(candidates))
}

fn collect_markdown_files(dir: &Path, target: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)
        .with_context(|| format!("failed to read directory {}", display_path(dir)))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_markdown_files(&path, target)?;
            continue;
        }

        if path
            .extension()
            .map(|extension| extension.eq_ignore_ascii_case("md"))
            .unwrap_or(false)
        {
            target.push(path);
        }
    }

    Ok(())
}

fn render_section(title: &str, blocks: Vec<String>) -> String {
    let mut body = format!("### {title}\n\n");

    if blocks.is_empty() {
        body.push_str("_(none)_\n");
        return body;
    }

    body.push_str(&blocks.join("\n\n"));
    body.push('\n');
    body
}

fn render_block(title: &str, body: &str) -> String {
    format!("#### {title}\n\n{}", body.trim())
}

fn unique(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();

    for value in values {
        if seen.insert(value.clone()) {
            ordered.push(value.clone());
        }
    }

    ordered
}

fn unique_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut ordered = Vec::new();

    for path in paths {
        let key = display_path(&path);
        if seen.insert(key) {
            ordered.push(path);
        }
    }

    ordered
}

fn extract_section_containing_marker(text: &str, marker: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let hit_index = lines.iter().position(|line| line.contains(marker))?;
    extract_section_around_index(&lines, hit_index)
}

fn extract_section_matching_heading(text: &str, needle: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let normalized_needle = normalize(needle);

    for (index, line) in lines.iter().enumerate() {
        if heading_level(line).is_some()
            && normalize(heading_text(line)).contains(&normalized_needle)
        {
            return extract_section_from_heading(&lines, index);
        }
    }

    None
}

fn extract_section_around_index(lines: &[&str], hit_index: usize) -> Option<String> {
    for index in (0..=hit_index).rev() {
        if heading_level(lines[index]).is_some() {
            return extract_section_from_heading(lines, index);
        }
    }

    let start = hit_index.saturating_sub(1);
    let end = usize::min(lines.len(), hit_index.saturating_add(3));
    Some(trimmed_join(&lines[start..end]))
}

fn extract_section_from_heading(lines: &[&str], heading_index: usize) -> Option<String> {
    let level = heading_level(lines[heading_index])?;
    let mut end = lines.len();

    for (index, line) in lines.iter().enumerate().skip(heading_index + 1) {
        if let Some(next_level) = heading_level(line)
            && next_level <= level
        {
            end = index;
            break;
        }
    }

    Some(trimmed_join(&lines[heading_index..end]))
}

fn heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    let count = trimmed.chars().take_while(|ch| *ch == '#').count();

    if count == 0
        || !trimmed
            .chars()
            .nth(count)
            .is_some_and(|ch| ch.is_whitespace())
    {
        return None;
    }

    Some(count)
}

fn heading_text(line: &str) -> &str {
    line.trim_start().trim_start_matches('#').trim()
}

fn trimmed_join(lines: &[&str]) -> String {
    let start = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(0);
    let end = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(lines.len());

    lines[start..end].join("\n")
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if ch.is_whitespace() || matches!(ch, '-' | '_') {
                Some(' ')
            } else {
                None
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
