use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use mutagen_service::auth::load_secret_from_process;
use mutagen_service::config::{self, ServiceConfig};
use mutagen_service::{app, shutdown_signal};
use tokio::net::TcpListener;

use mutagen_service::observability;

#[path = "auth/allowlist.rs"]
mod allowlist;
#[path = "auth/middleware.rs"]
mod middleware;
#[path = "auth/outcome.rs"]
mod outcome;

use middleware::auth_wrap;

const PROJECT_ROOT_ENV: &str = "MUTAGEN_PROJECT_ROOT";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let project_root = std::env::var_os(PROJECT_ROOT_ENV)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .context("could not determine project root for config lookup")?;

    let cfg: ServiceConfig =
        config::load_for_project(&project_root).context("service config refused startup")?;

    observability::init_tracing(&cfg.log_level);

    // ISC-003 / POL-A1: fail closed at bind time. No secret → no listener.
    let secret = Arc::new(
        load_secret_from_process(&cfg)
            .context("service refuses to bind without a configured shared secret")?,
    );

    let listener = TcpListener::bind(&cfg.listen)
        .await
        .with_context(|| format!("failed to bind {}", cfg.listen))?;

    let local = listener
        .local_addr()
        .context("listener has no local address")?;
    tracing::info!(
        addr = %local,
        log_level = %cfg.log_level,
        secret_path = %cfg.secret_path.display(),
        secret_id = %secret.secret_id(),
        "mutagen-service listening"
    );

    // Layer order matters: request-id is outermost so every line in the audit
    // log (including auth.accepted / auth.rejected emitted by `auth_wrap`) is
    // tagged with the same request_id span field.
    let router = observability::wrap(auth_wrap(app(), secret));

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("axum::serve exited with error")?;

    tracing::info!("mutagen-service drained and exiting");
    Ok(())
}
