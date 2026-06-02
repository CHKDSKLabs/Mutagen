//! HTTP route handlers. Every handler in this tree is signature-bound to a
//! type from `crate::dto::*` (INV-E4) — domain types never appear here.

pub mod projects;
pub mod session;
pub mod version;
pub mod workflow_read;
pub mod workflow_write;
