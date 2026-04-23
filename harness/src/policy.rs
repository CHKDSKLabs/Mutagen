use anyhow::{Result, bail};
use globset::{GlobBuilder, GlobMatcher};

use crate::queue::{Slice, SliceStatus};

pub fn author_stage_write_globs(slice: &Slice) -> Result<Vec<String>> {
    let mut globs = if slice.write_set.is_empty() {
        default_author_write_set(&slice.author_agent)?
    } else {
        slice.write_set.clone()
    };

    if slice.status == SliceStatus::BlockedRetry || slice.attempts > 0 {
        globs.extend(slice.adjacent_scope_allowed.clone());
    }

    globs.push("project_state.md".to_string());
    globs.push("infrastructure_state.md".to_string());
    globs.push(".mutagen/state/**".to_string());

    if !slice.context_to_update.trim().is_empty() {
        globs.push(slice.context_to_update.clone());
    }

    Ok(dedupe_globs(globs))
}

pub fn default_author_write_set(author_agent: &str) -> Result<Vec<String>> {
    let globs = match author_agent {
        "Bebop" => vec![
            "src/**",
            "app/**",
            "api/**",
            "components/**",
            "pages/**",
            "tests/**",
            "styles/**",
            "public/**",
        ],
        "Chaplin" => vec![
            "migrations/**",
            "schema/**",
            "db/**",
            "prisma/**",
            "src/models/**",
            "src/queries/**",
            "src/repositories/**",
            "seeds/**",
            "tests/db/**",
            "tests/migrations/**",
        ],
        "Metalhead" => vec![
            "observability/**",
            "dashboards/**",
            "alerts/**",
            "slo/**",
            "runbooks/alerts/**",
            "src/instrumentation/**",
            "src/tracing/**",
            "src/logging/**",
            "src/metrics/**",
            "src/telemetry/**",
            "tests/observability/**",
        ],
        "Splinter" => vec![
            "docs/api/**",
            "docs/onboarding/**",
            "docs/guides/**",
            "docs/how-to/**",
            "docs/architecture/**",
            "docs/migration/**",
            "docs/glossary.md",
            "runbooks/ops/**",
            "README.md",
            "CONTRIBUTING.md",
            "CHANGELOG.md",
        ],
        "Tatsu" => vec![
            "src/security/**",
            "src/auth/**",
            "middleware/**",
            "policies/**",
            "tests/security/**",
        ],
        "Krang" => vec![
            ".github/workflows/**",
            "fly.toml",
            "wrangler.toml",
            "Dockerfile",
            "docker-compose.*",
            "infrastructure/**",
            "terraform/**",
            "migrations/**",
            ".env.example",
        ],
        "Baxter" => {
            bail!(
                "slice requires an explicit write_set for Baxter; cited modules are not safely derivable"
            );
        }
        other => bail!("unknown author_agent `{other}`"),
    };

    Ok(globs.into_iter().map(str::to_string).collect())
}

pub fn globs_cover_all(globs: &[String], paths: &[String]) -> Result<bool> {
    if paths.is_empty() {
        return Ok(false);
    }

    let matchers = compile_glob_matchers(globs)?;

    Ok(paths.iter().all(|path| {
        let normalized = normalize_path(path);
        matchers.iter().any(|matcher| matcher.is_match(&normalized))
    }))
}

pub fn path_matches_any_glob(globs: &[String], path: &str) -> Result<bool> {
    let normalized = normalize_path(path);
    let matchers = compile_glob_matchers(globs)?;

    Ok(matchers.iter().any(|matcher| matcher.is_match(&normalized)))
}

pub fn first_matching_glob(globs: &[String], path: &str) -> Result<Option<String>> {
    let normalized = normalize_path(path);

    for glob in globs {
        let matcher = GlobBuilder::new(glob)
            .literal_separator(true)
            .build()?
            .compile_matcher();

        if matcher.is_match(&normalized) {
            return Ok(Some(glob.clone()));
        }
    }

    Ok(None)
}

fn compile_glob_matchers(globs: &[String]) -> Result<Vec<GlobMatcher>> {
    globs
        .iter()
        .map(|glob| {
            GlobBuilder::new(glob)
                .literal_separator(true)
                .build()
                .map(|compiled| compiled.compile_matcher())
                .map_err(Into::into)
        })
        .collect()
}

fn normalize_path(path: &str) -> String {
    path.trim().replace('\\', "/")
}

pub fn dedupe_globs(globs: Vec<String>) -> Vec<String> {
    let mut unique = Vec::new();

    for glob in globs {
        if !unique.contains(&glob) {
            unique.push(glob);
        }
    }

    unique
}
