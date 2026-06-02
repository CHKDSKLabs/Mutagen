use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "Repo automation tasks", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Regenerate docs/openapi.json from the live ApiDoc aggregator.
    Openapi {
        /// Override the output path. Defaults to <repo-root>/docs/openapi.json.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Openapi { out } => write_openapi(out),
    }
}

fn write_openapi(out: Option<PathBuf>) -> Result<()> {
    let path = match out {
        Some(p) => p,
        None => repo_root()?.join("docs").join("openapi.json"),
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent dir for {}", path.display()))?;
    }

    let bytes = mutagen_service::openapi::spec_json()?;
    std::fs::write(&path, bytes)
        .with_context(|| format!("writing openapi spec to {}", path.display()))?;

    println!("wrote {}", path.display());
    Ok(())
}

/// Walk up from CARGO_MANIFEST_DIR to find the workspace root. The xtask crate
/// lives at <repo>/xtask, so the repo root is one directory up.
fn repo_root() -> Result<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let root = PathBuf::from(manifest_dir)
        .parent()
        .map(PathBuf::from)
        .context("xtask manifest has no parent — running from an unexpected layout?")?;
    Ok(root)
}
