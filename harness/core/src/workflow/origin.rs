//! `Origin` value object for State Update records.
//!
//! ISC-007 / INV-W5: every State Update carries a non-empty origin
//! identifying the writer. We enforce this at the type level — the
//! writer signature takes `Origin`, not `&str`, so an empty literal
//! never crosses the boundary. The constructors here are the only
//! way to mint an `Origin`, and they reject empty session ids.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    Cli { pid: u32 },
    Service { session_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OriginError {
    EmptySessionId,
}

impl fmt::Display for OriginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OriginError::EmptySessionId => f.write_str(
                "service origin requires a non-empty session_id (ISC-007: empty origin is invalid)",
            ),
        }
    }
}

impl std::error::Error for OriginError {}

impl Origin {
    pub fn cli(pid: u32) -> Self {
        Origin::Cli { pid }
    }

    pub fn service(session_id: &str) -> Result<Self, OriginError> {
        let trimmed = session_id.trim();
        if trimmed.is_empty() {
            return Err(OriginError::EmptySessionId);
        }
        Ok(Origin::Service {
            session_id: trimmed.to_string(),
        })
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Origin::Cli { pid } => write!(f, "cli:{pid}"),
            Origin::Service { session_id } => write!(f, "service:{session_id}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_origin_is_infallible() {
        assert_eq!(Origin::cli(4242).to_string(), "cli:4242");
    }

    #[test]
    fn service_origin_rejects_empty_string() {
        assert!(matches!(
            Origin::service(""),
            Err(OriginError::EmptySessionId)
        ));
    }

    #[test]
    fn service_origin_rejects_whitespace_only() {
        assert!(matches!(
            Origin::service("   \t  "),
            Err(OriginError::EmptySessionId)
        ));
    }

    #[test]
    fn service_origin_accepts_real_id() {
        let o = Origin::service("sess-abc-123").expect("non-empty session id is valid");
        assert_eq!(o.to_string(), "service:sess-abc-123");
    }

    #[test]
    fn origin_round_trips_through_json() {
        let cli = Origin::cli(7);
        let raw = serde_json::to_string(&cli).unwrap();
        let back: Origin = serde_json::from_str(&raw).unwrap();
        assert_eq!(cli, back);
    }
}
