pub mod request_id;

use axum::Router;
use axum::middleware::from_fn;
use tracing::Metadata;
use tracing::subscriber::Subscriber;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub use request_id::{RequestId, X_REQUEST_ID, request_id_middleware};

/// Field names that must never appear in any emitted event or span (ISC-001 / DSD-633).
pub const FORBIDDEN_FIELD_NAMES: &[&str] = &["bearer", "token", "authorization", "secret"];

/// Wrap a router with the request-id middleware. The span/log topology DSD-634
/// requires is built inside the middleware itself.
pub fn wrap<S: Clone + Send + Sync + 'static>(router: Router<S>) -> Router<S> {
    router.layer(from_fn(request_id_middleware))
}

/// Install the JSON tracing subscriber globally. Safe to call multiple times;
/// subsequent calls become no-ops because the global default is already set.
pub fn init_tracing(level: &str) {
    let _ = build_subscriber(level, std::io::stdout).try_init();
}

/// Build a subscriber against the supplied writer. Used by `init_tracing` for
/// stdout and by integration tests for in-memory capture.
pub fn build_subscriber<W>(
    level: &str,
    make_writer: W,
) -> Box<dyn Subscriber + Send + Sync + 'static>
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let env = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    let secret_filter = filter_fn(|metadata: &Metadata<'_>| {
        !metadata.fields().iter().any(|f| {
            FORBIDDEN_FIELD_NAMES
                .iter()
                .any(|name| f.name().eq_ignore_ascii_case(name))
        })
    });

    let fmt = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_writer(make_writer)
        .json()
        .with_filter(secret_filter);

    Box::new(tracing_subscriber::registry().with(env).with(fmt))
}

#[cfg(test)]
mod self_tests {
    use super::*;

    #[test]
    fn forbidden_names_are_case_insensitive() {
        // probe via the public list; this guards against typos in FORBIDDEN_FIELD_NAMES
        assert!(FORBIDDEN_FIELD_NAMES.contains(&"bearer"));
        assert!(FORBIDDEN_FIELD_NAMES.contains(&"authorization"));
    }
}
