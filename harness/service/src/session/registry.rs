//! Active-session registry — the in-memory lock that backs ISC-015.
//!
//! Keyed by `project_id`; insertion under the inner Mutex is the linearisation
//! point for "at most one active Session per project". A successful
//! [`try_acquire`] hands back a [`SessionLock`] RAII guard whose Drop releases
//! the slot (INV-S5 / POL-P2) — drop the guard, the seat opens.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct ActiveSessionRegistry {
    inner: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Debug)]
pub enum AcquireOutcome {
    Acquired(SessionLock),
    Conflict { active_session_id: String },
}

/// RAII guard. Drop releases the seat — INV-S5 / POL-P2 are upheld here.
#[derive(Debug)]
pub struct SessionLock {
    registry: Arc<Mutex<HashMap<String, String>>>,
    project_id: String,
    session_id: String,
}

impl SessionLock {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        // Only release if the slot still names *us*. Belt-and-suspenders against
        // any future code that might race a manual release in.
        let mut map = match self.registry.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(holder) = map.get(&self.project_id)
            && holder == &self.session_id
        {
            map.remove(&self.project_id);
        }
    }
}

impl Default for ActiveSessionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ActiveSessionRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Try to claim the active-session slot for `project_id`. On success the
    /// caller receives a [`SessionLock`] that releases on Drop. On conflict
    /// the active holder's `session_id` is returned so the 409 envelope can
    /// quote it back (ISC-015 detection vector).
    pub fn try_acquire(&self, project_id: &str, session_id: &str) -> AcquireOutcome {
        let mut map = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some(active) = map.get(project_id) {
            return AcquireOutcome::Conflict {
                active_session_id: active.clone(),
            };
        }
        map.insert(project_id.to_owned(), session_id.to_owned());
        AcquireOutcome::Acquired(SessionLock {
            registry: Arc::clone(&self.inner),
            project_id: project_id.to_owned(),
            session_id: session_id.to_owned(),
        })
    }

    /// Inspect the current active session for a project. Returns the seated
    /// `session_id` if one is held. Used by tests and 409 envelope building.
    pub fn is_active(&self, project_id: &str) -> Option<String> {
        let map = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        map.get(project_id).cloned()
    }
}

#[cfg(test)]
mod self_tests {
    use super::*;

    #[test]
    fn first_acquire_wins() {
        let r = ActiveSessionRegistry::new();
        match r.try_acquire("proj-a", "sess-1") {
            AcquireOutcome::Acquired(_) => {}
            other => panic!("expected Acquired, got {other:?}"),
        }
    }

    #[test]
    fn second_acquire_reports_active_id() {
        let r = ActiveSessionRegistry::new();
        let _g = match r.try_acquire("proj-a", "sess-1") {
            AcquireOutcome::Acquired(g) => g,
            other => panic!("first must acquire, got {other:?}"),
        };
        match r.try_acquire("proj-a", "sess-2") {
            AcquireOutcome::Conflict { active_session_id } => {
                assert_eq!(active_session_id, "sess-1");
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
    }

    #[test]
    fn drop_releases_seat() {
        let r = ActiveSessionRegistry::new();
        {
            let _g = match r.try_acquire("proj-a", "sess-1") {
                AcquireOutcome::Acquired(g) => g,
                _ => panic!("first acquire failed"),
            };
            assert_eq!(r.is_active("proj-a").as_deref(), Some("sess-1"));
        }
        assert!(r.is_active("proj-a").is_none(), "Drop must release");
        match r.try_acquire("proj-a", "sess-2") {
            AcquireOutcome::Acquired(_) => {}
            other => panic!("post-release re-acquire failed: {other:?}"),
        }
    }

    #[test]
    fn distinct_projects_dont_collide() {
        let r = ActiveSessionRegistry::new();
        let _a = match r.try_acquire("proj-a", "sess-1") {
            AcquireOutcome::Acquired(g) => g,
            _ => panic!("a acquire failed"),
        };
        match r.try_acquire("proj-b", "sess-2") {
            AcquireOutcome::Acquired(_) => {}
            other => panic!("proj-b should acquire independently: {other:?}"),
        }
    }
}
