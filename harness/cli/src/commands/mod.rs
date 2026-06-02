//! CLI command helpers. Orphan module today — not yet referenced from
//! main.rs because main.rs is outside this slice's write scope. Future
//! re-dispatch should add `mod commands;` and route the State Update
//! call sites (advance / finalize / escalate / resume) through
//! [`state_log::cli_origin`] so every CLI-authored State Update lands
//! with `origin = cli:<pid>` per ISC-007.

pub mod state_log;
