use mutagen_core::adapter::{ParallelDispatchMode, ScopeEnforcementMode, resolved_host_profile};
use mutagen_core::config::WorkflowConfig;

#[test]
fn codex_profile_degrades_parallel_dispatch_when_worktrees_are_unavailable() {
    let workflow_config = WorkflowConfig {
        max_parallel_slices: 4,
        ..WorkflowConfig::default()
    };

    let profile = resolved_host_profile(mutagen_core::adapter::HostKind::Codex, &workflow_config);

    assert_eq!(profile.scope_enforcement, ScopeEnforcementMode::Advisory);
    assert_eq!(profile.parallel_dispatch, ParallelDispatchMode::SerialOnly);
    assert_eq!(profile.requested_max_parallel_slices, 4);
    assert_eq!(profile.effective_max_parallel_slices, 1);
    assert!(
        profile
            .degraded_features
            .contains(&"parallel_dispatch".to_string())
    );
    assert!(
        profile
            .downgrades
            .iter()
            .any(|downgrade| downgrade.feature == "parallel_dispatch")
    );
}

#[test]
fn claude_profile_preserves_bounded_parallel_dispatch_when_requested() {
    let workflow_config = WorkflowConfig {
        max_parallel_slices: 4,
        ..WorkflowConfig::default()
    };

    let profile = resolved_host_profile(mutagen_core::adapter::HostKind::Claude, &workflow_config);

    assert_eq!(profile.scope_enforcement, ScopeEnforcementMode::Hard);
    assert_eq!(
        profile.parallel_dispatch,
        ParallelDispatchMode::BoundedCohort
    );
    assert_eq!(profile.requested_max_parallel_slices, 4);
    assert_eq!(profile.effective_max_parallel_slices, 4);
    assert!(
        !profile
            .degraded_features
            .contains(&"parallel_dispatch".to_string())
    );
}
