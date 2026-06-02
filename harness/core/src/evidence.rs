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
            .ok_or_else(|| citation_resolution_error("PRD", &citation))?;
            Ok(render_block(&citation, &excerpt))
        })
        .collect::<Result<Vec<_>>>()?;

    // Try the per-file layout first (one ADR-XXX.md per decision); if a citation
    // doesn't resolve there, fall back to a consolidated docs/ADR.md or ADR.md and
    // pull the section out by heading like we do for DDD/ISC/DSD.
    let consolidated_adr = load_consolidated_adr(workspace_root)?;
    let adr_blocks = unique(&slice.traces_to.adr)
        .into_iter()
        .map(|citation| {
            if let Ok(path) = resolve_adr_path(workspace_root, &citation) {
                let body = fs::read_to_string(&path).with_context(|| {
                    format!("failed to read ADR file at {}", display_path(&path))
                })?;
                return Ok(render_block(&citation, body.trim()));
            }

            if let Some(doc) = consolidated_adr.as_deref() {
                let excerpt = extract_section_matching_heading(doc, &citation)
                    .or_else(|| extract_section_containing_marker(doc, &citation))
                    .ok_or_else(|| citation_resolution_error("ADR", &citation))?;
                return Ok(render_block(&citation, &excerpt));
            }

            Err(citation_resolution_error("ADR", &citation))
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
                    .ok_or_else(|| citation_resolution_error("DDD", &citation))?;
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
            .ok_or_else(|| citation_resolution_error("ISC", &citation))?;
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
            .ok_or_else(|| citation_resolution_error("DSD", &citation))?;
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

fn load_consolidated_adr(workspace_root: &Path) -> Result<Option<String>> {
    let candidates = [
        workspace_root.join("docs").join("ADR.md"),
        workspace_root.join("ADR.md"),
    ];

    for path in candidates {
        if path.is_file() {
            let body = fs::read_to_string(&path).with_context(|| {
                format!(
                    "failed to read consolidated ADR doc at {}",
                    display_path(&path)
                )
            })?;
            return Ok(Some(body));
        }
    }

    Ok(None)
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

    for candidate in literal_candidates(marker) {
        if candidate.is_empty() {
            continue;
        }
        if let Some(hit_index) = lines.iter().position(|line| line.contains(&candidate)) {
            return extract_section_around_index(&lines, hit_index);
        }
    }

    // Last-ditch normalized substring search. Only fires when literal forms above all
    // missed -- gives us slack when the doc's punctuation drifts from the citation.
    // The 2-word floor stops single tokens from drive-by matching unrelated lines.
    let normalized = normalize(marker);
    if normalized.split_whitespace().count() >= 2
        && let Some(hit_index) = lines.iter().position(|line| {
            let line_norm = normalize(line);
            !line_norm.is_empty() && line_norm.contains(&normalized)
        })
    {
        return extract_section_around_index(&lines, hit_index);
    }

    None
}

fn extract_section_matching_heading(text: &str, needle: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut needles: Vec<String> = literal_candidates(needle)
        .into_iter()
        .map(|candidate| normalize(&candidate))
        .filter(|candidate| !candidate.is_empty())
        .collect();
    needles.dedup();

    for (index, line) in lines.iter().enumerate() {
        if heading_level(line).is_none() {
            continue;
        }
        let heading = normalize(heading_text(line));
        if heading.is_empty() {
            continue;
        }

        for needle_norm in &needles {
            // Original direction -- the historical behaviour; keep single-word matches valid.
            if heading.contains(needle_norm) {
                return extract_section_from_heading(&lines, index);
            }
            // Reverse direction handles a long descriptive citation wrapping around a
            // shorter heading. Floor on heading side prevents single-token headings from
            // grabbing every citation that happens to mention them.
            if heading.split_whitespace().count() >= 2 && needle_norm.contains(&heading) {
                return extract_section_from_heading(&lines, index);
            }
        }
    }

    None
}

fn literal_candidates(citation: &str) -> Vec<String> {
    let trimmed = citation.trim().to_string();
    let canonical = canonicalize_citation(&trimmed);

    let mut out = Vec::with_capacity(2);
    out.push(trimmed.clone());
    if !canonical.is_empty() && canonical != trimmed {
        out.push(canonical);
    }
    out
}

// Trims the descriptive crud Shredder likes to bolt onto traces_to entries:
// role-prefixes ("Cross-cutting:", "NFR:"), leading section markers ("§4 ", "§4.2 "),
// and trailing parentheticals (" (§4 note: Tool is render-only)"). Returns whatever's
// left so the resolver can match against the actual heading text in the doc.
fn canonicalize_citation(citation: &str) -> String {
    let mut s = citation.trim().to_string();

    if let Some(colon_at) = s.find(": ") {
        let prefix = &s[..colon_at];
        let words: Vec<&str> = prefix.split_whitespace().collect();
        let looks_like_role = !words.is_empty()
            && words.len() <= 2
            && words
                .iter()
                .all(|word| word.chars().all(|ch| ch.is_alphanumeric() || ch == '-'));
        if looks_like_role {
            s = s[colon_at + 2..].trim_start().to_string();
        }
    }

    if let Some(rest) = strip_leading_section_marker(&s) {
        s = rest;
    }

    while s.ends_with(')') {
        if let Some(open_at) = s.rfind(" (") {
            s.truncate(open_at);
        } else {
            break;
        }
    }

    s.trim().to_string()
}

fn strip_leading_section_marker(s: &str) -> Option<String> {
    let rest = s.strip_prefix('§')?;
    let split_at = rest
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_digit() && *ch != '.')
        .map(|(idx, _)| idx)
        .unwrap_or(rest.len());

    if split_at == 0 {
        return None;
    }

    let after = &rest[split_at..];
    let trimmed = after.trim_start();
    if trimmed.len() == after.len() {
        return None;
    }

    Some(trimmed.to_string())
}

fn citation_resolution_error(kind: &str, citation: &str) -> anyhow::Error {
    let canonical = canonicalize_citation(citation);
    let trimmed = citation.trim();
    if !canonical.is_empty() && canonical != trimmed {
        anyhow::anyhow!(
            "failed to resolve {kind} citation `{citation}` (also tried canonicalized form `{canonical}`)"
        )
    } else {
        anyhow::anyhow!("failed to resolve {kind} citation `{citation}`")
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_strips_trailing_parenthetical() {
        let raw = "World/Render context split (§4 note: Tool is render-only)";
        assert_eq!(canonicalize_citation(raw), "World/Render context split");
    }

    #[test]
    fn canonicalize_strips_role_prefix() {
        assert_eq!(
            canonicalize_citation("Cross-cutting: Single-source ground pick"),
            "Single-source ground pick"
        );
        assert_eq!(
            canonicalize_citation("NFR: Latency budget"),
            "Latency budget"
        );
    }

    #[test]
    fn canonicalize_strips_leading_section_marker() {
        assert_eq!(
            canonicalize_citation("§4 World/Render context split"),
            "World/Render context split"
        );
        assert_eq!(
            canonicalize_citation("§4.2 World/Render context split"),
            "World/Render context split"
        );
    }

    #[test]
    fn canonicalize_combines_strip_passes() {
        let raw = "Cross-cutting: §4.2 World/Render context split (note: aside)";
        assert_eq!(canonicalize_citation(raw), "World/Render context split");
    }

    #[test]
    fn canonicalize_leaves_clean_citations_alone() {
        assert_eq!(
            canonicalize_citation("Plain heading text"),
            "Plain heading text"
        );
    }

    #[test]
    fn canonicalize_does_not_strip_multiword_or_punctuated_prefix() {
        // Prefix has slash punctuation -- not a role marker, leave it.
        let raw = "World/Render context: split detail";
        assert_eq!(canonicalize_citation(raw), raw);

        // Three-word prefix is too long to be a role marker.
        let raw = "Section four point two: rest of title";
        assert_eq!(canonicalize_citation(raw), raw);
    }

    #[test]
    fn canonicalize_handles_multiple_trailing_parens() {
        assert_eq!(
            canonicalize_citation("Title (note one) (note two)"),
            "Title"
        );
    }

    #[test]
    fn extract_section_finds_via_canonical_form() {
        let doc = "## Intro\n\nText.\n\n## World/Render context split\n\nBody of the section.\n\n## Other\n\nx";
        let citation = "World/Render context split (§4 note: Tool is render-only)";
        let excerpt = extract_section_containing_marker(doc, citation)
            .expect("expected canonical form to resolve");
        assert!(excerpt.contains("Body of the section"));
        assert!(excerpt.starts_with("## World/Render context split"));
    }

    #[test]
    fn extract_heading_match_is_bidirectional() {
        let doc = "## Single-source ground pick\n\nA short note.\n";
        let citation = "Cross-cutting: Single-source ground pick";
        let excerpt =
            extract_section_matching_heading(doc, citation).expect("heading match should hit");
        assert!(excerpt.contains("A short note"));

        // And the inverse: a heading that decorates around the citation.
        let doc = "## Detailed: Single-source ground pick (subsystem)\n\nBody.\n";
        let citation = "Single-source ground pick";
        let excerpt = extract_section_matching_heading(doc, citation)
            .expect("over-decorated heading should still match");
        assert!(excerpt.contains("Body"));
    }

    #[test]
    fn extract_preserves_single_word_forward_match() {
        // Original-direction matches still work even for single-word needles -- the
        // resolver finds the first heading whose normalized form contains the needle.
        let doc = "## Foo\n\nshort.\n\n## Foo bar baz\n\nlong.\n";
        let excerpt = extract_section_matching_heading(doc, "Foo")
            .expect("forward single-word match should still resolve");
        assert!(excerpt.starts_with("## Foo"));
    }

    #[test]
    fn extract_reverse_direction_requires_multiword_heading() {
        // Long descriptive citation, single-word heading: the reverse-direction match
        // would technically fire (heading "abc" is contained in needle "abc def ghi"),
        // but the floor blocks it. Without the floor, every short heading in the doc
        // would grab unrelated long citations.
        let doc = "## Tools\n\nshort.\n";
        let citation = "Tools and infrastructure for ground-pick";
        let excerpt = extract_section_matching_heading(doc, citation);
        assert!(
            excerpt.is_none(),
            "single-word heading must not reverse-match a long descriptive citation"
        );
    }

    #[test]
    fn citation_resolution_error_includes_canonical_form() {
        let err = citation_resolution_error("DDD", "Cross-cutting: Single-source ground pick");
        let msg = err.to_string();
        assert!(msg.contains("Cross-cutting: Single-source ground pick"));
        assert!(msg.contains("Single-source ground pick"));
        assert!(msg.contains("canonicalized form"));
    }

    #[test]
    fn load_consolidated_adr_finds_docs_or_root() {
        let tmp = std::env::temp_dir().join(format!(
            "mutagen-evidence-adr-fallback-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(tmp.join("docs")).unwrap();
        std::fs::write(tmp.join("docs/ADR.md"), "# ADR\n\n## ADR-001 X\n\nbody\n").unwrap();

        let body = load_consolidated_adr(&tmp).unwrap().unwrap();
        assert!(body.contains("ADR-001 X"));

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn load_consolidated_adr_returns_none_when_absent() {
        let tmp = std::env::temp_dir().join(format!(
            "mutagen-evidence-adr-absent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(load_consolidated_adr(&tmp).unwrap().is_none());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn citation_resolution_error_omits_canonical_when_unchanged() {
        let err = citation_resolution_error("PRD", "Plain heading text");
        let msg = err.to_string();
        assert!(!msg.contains("canonicalized form"));
        assert!(msg.contains("Plain heading text"));
    }
}
