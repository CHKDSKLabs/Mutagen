//! L3-Auth-002 — bearer-auth middleware + verifier.
//!
//! Slots between the request-id layer (outer) and the route handlers (inner)
//! so every non-allowlisted route is forced through `verify_request` before
//! a handler ever sees the request. POL-A1 / ISC-003: any error path produces
//! `Reject`, never `Accept`. INV-A4 / ISC-001: the 401 envelope reveals nothing
//! about which check failed, and the bearer bytes never reach a log line.

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use mutagen_service::auth::{AuthRejectionReason, BearerToken, Secret};

use super::allowlist::is_unauthenticated;
use super::outcome::{AuthOutcome, Principal};

/// DSD-624 error envelope. `details` is omitted on auth failures by design —
/// INV-A4 says the response MUST NOT reveal why a verification rejected.
#[derive(Serialize)]
struct ErrorEnvelope {
    code: &'static str,
    message: &'static str,
    request_id: String,
}

/// Apply the bearer-auth layer to a router. Every route — including the WS
/// upgrade route per NFR-2 — that is not on the allowlist must produce a
/// successful `AuthOutcome::Accept` before its handler is invoked.
pub fn auth_wrap<S>(router: Router<S>, secret: Arc<Secret>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let layer_secret = secret;
    router.layer(from_fn(move |req: Request<Body>, next: Next| {
        let secret = Arc::clone(&layer_secret);
        async move { auth_inner(secret, req, next).await }
    }))
}

/// Pure verification step. Returns `AuthOutcome`, *not* `bool` or
/// `Result<bool, _>` — ISC-003's escape hatch is closed at the type level.
pub fn verify_request(raw_header: Option<&[u8]>, secret: &Secret) -> AuthOutcome {
    let Some(bytes) = raw_header else {
        return AuthOutcome::Reject(AuthRejectionReason::MissingHeader);
    };

    let Ok(text) = std::str::from_utf8(bytes) else {
        return AuthOutcome::Reject(AuthRejectionReason::MalformedHeader);
    };

    let trimmed = text.trim();
    let Some((scheme, rest)) = trimmed.split_once(' ') else {
        return AuthOutcome::Reject(AuthRejectionReason::MalformedHeader);
    };

    if !scheme.eq_ignore_ascii_case("Bearer") {
        return AuthOutcome::Reject(AuthRejectionReason::UnknownScheme);
    }

    let candidate = rest.trim();
    if candidate.is_empty() {
        return AuthOutcome::Reject(AuthRejectionReason::MalformedHeader);
    }

    let token = BearerToken::from_bytes(candidate.as_bytes().to_vec());
    if bool::from(token.verify_against(secret)) {
        AuthOutcome::Accept(Principal {
            principal_id: format!("secret:{}", secret.secret_id()),
        })
    } else {
        AuthOutcome::Reject(AuthRejectionReason::SecretMismatch)
    }
}

async fn auth_inner(secret: Arc<Secret>, req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path();
    if is_unauthenticated(path) {
        return next.run(req).await;
    }

    let raw = req
        .headers()
        .get(header::AUTHORIZATION)
        .map(|v| v.as_bytes());

    let outcome = verify_request(raw, secret.as_ref());

    match outcome {
        AuthOutcome::Accept(principal) => {
            tracing::info!(
                event = "auth.accepted",
                secret_id = %secret.secret_id(),
                principal_id = %principal.principal_id,
                "auth.accepted",
            );
            next.run(req).await
        }
        AuthOutcome::Reject(reason) => {
            tracing::info!(
                event = "auth.rejected",
                secret_id = %secret.secret_id(),
                reason = ?reason,
                "auth.rejected",
            );
            let request_id = req
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned())
                .unwrap_or_default();
            unauth_response(request_id)
        }
    }
}

fn unauth_response(request_id: String) -> Response {
    let body = ErrorEnvelope {
        code: "UNAUTHENTICATED",
        message: "authentication required",
        request_id,
    };
    (StatusCode::UNAUTHORIZED, Json(body)).into_response()
}

#[cfg(test)]
mod self_tests {
    use super::*;

    fn fake_secret(bytes: &[u8]) -> Secret {
        Secret::new(bytes.to_vec(), "test:fixed".to_owned())
    }

    #[test]
    fn missing_header_rejects() {
        let s = fake_secret(b"hunter2");
        match verify_request(None, &s) {
            AuthOutcome::Reject(AuthRejectionReason::MissingHeader) => {}
            other => panic!("expected MissingHeader, got {other:?}"),
        }
    }

    #[test]
    fn bad_utf8_header_rejects_malformed() {
        let s = fake_secret(b"hunter2");
        match verify_request(Some(&[0xff, 0xfe, 0xfd]), &s) {
            AuthOutcome::Reject(AuthRejectionReason::MalformedHeader) => {}
            other => panic!("expected MalformedHeader, got {other:?}"),
        }
    }

    #[test]
    fn wrong_scheme_rejects_unknown_scheme() {
        let s = fake_secret(b"hunter2");
        match verify_request(Some(b"Basic dXNlcjpwYXNz"), &s) {
            AuthOutcome::Reject(AuthRejectionReason::UnknownScheme) => {}
            other => panic!("expected UnknownScheme, got {other:?}"),
        }
    }

    #[test]
    fn empty_bearer_value_rejects_malformed() {
        let s = fake_secret(b"hunter2");
        match verify_request(Some(b"Bearer    "), &s) {
            AuthOutcome::Reject(AuthRejectionReason::MalformedHeader) => {}
            other => panic!("expected MalformedHeader, got {other:?}"),
        }
    }

    #[test]
    fn mismatch_rejects_secret_mismatch() {
        let s = fake_secret(b"hunter2");
        match verify_request(Some(b"Bearer nope"), &s) {
            AuthOutcome::Reject(AuthRejectionReason::SecretMismatch) => {}
            other => panic!("expected SecretMismatch, got {other:?}"),
        }
    }

    #[test]
    fn exact_match_accepts() {
        let s = fake_secret(b"hunter2");
        match verify_request(Some(b"Bearer hunter2"), &s) {
            AuthOutcome::Accept(p) => assert!(p.principal_id.starts_with("secret:")),
            other => panic!("expected Accept, got {other:?}"),
        }
    }

    #[test]
    fn scheme_match_is_case_insensitive() {
        let s = fake_secret(b"hunter2");
        match verify_request(Some(b"bearer hunter2"), &s) {
            AuthOutcome::Accept(_) => {}
            other => panic!("expected Accept (case-insensitive scheme), got {other:?}"),
        }
        match verify_request(Some(b"BEARER hunter2"), &s) {
            AuthOutcome::Accept(_) => {}
            other => panic!("expected Accept (uppercase scheme), got {other:?}"),
        }
    }
}
