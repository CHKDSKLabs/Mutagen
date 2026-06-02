//! L3-Auth-002 — unauthenticated-routes allowlist.
//!
//! ISC-011 + DSD-303 + POL-E2: this constant is the *only* place that can
//! widen the set of routes reachable without a bearer secret. No env var, no
//! config flag, no feature flag may extend it — adding a path means editing
//! this file in a reviewed PR, on purpose, with eyes open.

pub const UNAUTHENTICATED: &[&str] = &["/health", "/version", "/openapi.json"];

pub fn is_unauthenticated(path: &str) -> bool {
    UNAUTHENTICATED.contains(&path)
}

#[cfg(test)]
mod self_tests {
    use super::*;

    #[test]
    fn allowlist_is_exactly_three_paths() {
        assert_eq!(UNAUTHENTICATED.len(), 3);
        assert_eq!(UNAUTHENTICATED[0], "/health");
        assert_eq!(UNAUTHENTICATED[1], "/version");
        assert_eq!(UNAUTHENTICATED[2], "/openapi.json");
    }

    #[test]
    fn arbitrary_paths_are_authenticated() {
        assert!(!is_unauthenticated("/projects"));
        assert!(!is_unauthenticated("/projects/abc"));
        assert!(!is_unauthenticated("/healthz"));
        assert!(!is_unauthenticated("/health/"));
        assert!(!is_unauthenticated(""));
    }

    #[test]
    fn allowlisted_paths_match_exact() {
        assert!(is_unauthenticated("/health"));
        assert!(is_unauthenticated("/version"));
        assert!(is_unauthenticated("/openapi.json"));
    }
}
