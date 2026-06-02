//! L3-Auth-002 — auth verification outcome.
//!
//! INV-A1: verification is binary, accept or reject, no third state. The type
//! itself forecloses ambiguity — there is no `bool`, no `Result<bool, _>`, no
//! `Option<Principal>` for a fallible verifier to wedge an accident through.

use mutagen_service::auth::AuthRejectionReason;

#[derive(Debug, Clone)]
pub struct Principal {
    pub principal_id: String,
}

#[derive(Debug)]
pub enum AuthOutcome {
    Accept(Principal),
    Reject(AuthRejectionReason),
}
