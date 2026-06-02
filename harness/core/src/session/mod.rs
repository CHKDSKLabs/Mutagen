//! Session bounded context — DDD §3.3.
//!
//! Hosts the [`Session`] aggregate and the chat protocol version constant.
//! Per ISC-012 Sessions are pure in-memory aggregates; no persistence layer
//! is wired here on purpose. INV-S2 monotonicity is enforced at the type
//! boundary — transitions go through methods, no public field mutation.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod answer;
pub mod envelope;

pub use answer::{Answer, AnswerValidationError, append_user_reply, validate as validate_answer};
pub use envelope::{QuestionEnvelope, QuestionKind};

/// MD-5 / ISC-009: the wire schema version advertised by the chat protocol.
/// Bumps go in lockstep with the Question Envelope shape; downstream clients
/// pin to a version range they understand.
pub const CHAT_PROTOCOL_VERSION: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Opened,
    Authenticated,
    Active,
    Closing,
    Closed,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: String,
    pub project_id: String,
    pub principal_id: String,
    pub status: SessionStatus,
}

#[derive(Debug)]
pub enum TransitionError {
    /// INV-S2 — can't claw back into an earlier state.
    Monotonic {
        from: SessionStatus,
        to: SessionStatus,
    },
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Monotonic { from, to } => {
                write!(f, "non-monotonic session transition: {from:?} → {to:?}")
            }
        }
    }
}

impl std::error::Error for TransitionError {}

impl Session {
    /// Construct a freshly-opened Session. `session_id` is server-minted UUIDv7
    /// per DDD §3.3 / DSD-623.
    pub fn open(project_id: String, principal_id: String) -> Self {
        Self {
            session_id: Uuid::now_v7().to_string(),
            project_id,
            principal_id,
            status: SessionStatus::Opened,
        }
    }

    pub fn authenticate(&mut self) -> Result<(), TransitionError> {
        self.transition(SessionStatus::Authenticated)
    }

    pub fn activate(&mut self) -> Result<(), TransitionError> {
        self.transition(SessionStatus::Active)
    }

    /// Begin a graceful close. Idempotent if already closing or closed
    /// (POL-S2: repeated close is a no-op).
    pub fn begin_close(&mut self) -> Result<(), TransitionError> {
        match self.status {
            SessionStatus::Closing | SessionStatus::Closed => Ok(()),
            _ => self.transition(SessionStatus::Closing),
        }
    }

    /// Terminate. Idempotent on a closed Session (POL-S2).
    pub fn close(&mut self) -> Result<(), TransitionError> {
        if matches!(self.status, SessionStatus::Closed) {
            return Ok(());
        }
        self.transition(SessionStatus::Closed)
    }

    fn transition(&mut self, to: SessionStatus) -> Result<(), TransitionError> {
        use SessionStatus::*;
        let allowed = matches!(
            (self.status, to),
            (Opened, Authenticated)
                | (Authenticated, Active)
                | (Active, Closing)
                | (Closing, Closed)
                | (Active, Closed)
                | (Opened, Closing)
                | (Opened, Closed)
                | (Authenticated, Closing)
                | (Authenticated, Closed)
        );
        if !allowed {
            return Err(TransitionError::Monotonic {
                from: self.status,
                to,
            });
        }
        self.status = to;
        Ok(())
    }
}

#[cfg(test)]
mod self_tests {
    use super::*;

    #[test]
    fn chat_protocol_version_is_v1() {
        assert_eq!(CHAT_PROTOCOL_VERSION, "1");
    }

    #[test]
    fn session_id_is_uuidv7_shaped() {
        let s = Session::open("p".into(), "secret:t".into());
        assert_eq!(s.session_id.len(), 36);
        // version nibble lives at byte index 14 of canonical UUID string
        assert_eq!(s.session_id.as_bytes()[14], b'7');
        assert_eq!(s.status, SessionStatus::Opened);
    }

    #[test]
    fn monotonic_forward_progress() {
        let mut s = Session::open("p".into(), "principal".into());
        s.authenticate().unwrap();
        s.activate().unwrap();
        s.begin_close().unwrap();
        s.close().unwrap();
        assert_eq!(s.status, SessionStatus::Closed);
    }

    #[test]
    fn closed_cannot_reopen() {
        let mut s = Session::open("p".into(), "principal".into());
        s.authenticate().unwrap();
        s.activate().unwrap();
        s.close().unwrap();
        assert!(s.authenticate().is_err());
        assert!(s.activate().is_err());
    }

    #[test]
    fn repeated_close_is_noop() {
        let mut s = Session::open("p".into(), "principal".into());
        s.authenticate().unwrap();
        s.activate().unwrap();
        s.close().unwrap();
        s.close().unwrap();
        assert_eq!(s.status, SessionStatus::Closed);
    }
}
