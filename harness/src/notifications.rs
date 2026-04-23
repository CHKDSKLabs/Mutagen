use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NotificationEvent {
    pub event: NotificationKind,
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    Escalation,
    StructuralFail,
    ScopeViolation,
    QueueClear,
    UserInterrupt,
    LayerComplete,
    Milestone,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopCondition {
    QueueClear,
    QueueStalled,
    StructuralFailure,
    ScopeViolation,
    RetryBudgetExhausted,
}

pub fn queue_clear_notification(completed_count: usize) -> NotificationEvent {
    NotificationEvent {
        event: NotificationKind::QueueClear,
        title: "mutagen — queue clear".to_string(),
        message: format!("{completed_count} slices completed."),
    }
}

pub fn layer_complete_notification(
    layer: u32,
    completed_in_layer: usize,
    next_pending_slice_id: Option<&str>,
) -> NotificationEvent {
    NotificationEvent {
        event: NotificationKind::LayerComplete,
        title: format!("mutagen — layer {layer} complete"),
        message: format!(
            "{completed_in_layer} slices completed in layer {layer}. Next pending slice: {}",
            next_pending_slice_id.unwrap_or("queue clear")
        ),
    }
}

pub fn retry_exhausted_notification(
    slice_id: &str,
    title: &str,
    attempts: u32,
    micro_corrections_used: u32,
) -> NotificationEvent {
    NotificationEvent {
        event: NotificationKind::Escalation,
        title: format!("mutagen — halted at {slice_id}"),
        message: format!(
            "{slice_id} ({title}) escalated after {attempts} attempts ({micro_corrections_used} micro-corrections). Blocked by: Tiger Claw. Needs human input."
        ),
    }
}

pub fn structural_fail_notification(
    slice_id: &str,
    title: &str,
    first_finding_detail: &str,
) -> NotificationEvent {
    NotificationEvent {
        event: NotificationKind::StructuralFail,
        title: format!("mutagen — structural fail on {slice_id}"),
        message: format!(
            "Structural check halted {slice_id} ({title}): {first_finding_detail}. Needs human input."
        ),
    }
}

pub fn scope_violation_notification(
    slice_id: Option<&str>,
    path: &str,
    class: &str,
    stage: Option<&str>,
    active_agent: Option<&str>,
) -> NotificationEvent {
    let slice_id = slice_id.unwrap_or("unknown_slice");
    let stage = stage.unwrap_or("unknown");
    let active_agent = active_agent.unwrap_or("unknown");

    NotificationEvent {
        event: NotificationKind::ScopeViolation,
        title: format!("mutagen — scope violation on {slice_id}"),
        message: format!(
            "Traag DENY on {path} (class: {class}) during stage {stage}. Agent: {active_agent}."
        ),
    }
}
