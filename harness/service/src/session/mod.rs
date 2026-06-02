//! Session bounded context (service-side) — owns the in-memory active-session
//! lock that backs ISC-015 enforcement. The Session aggregate itself lives in
//! [`mutagen_core::session`]; this module is the service-edge plumbing only.

pub mod registry;

pub use registry::{AcquireOutcome, ActiveSessionRegistry, SessionLock};
