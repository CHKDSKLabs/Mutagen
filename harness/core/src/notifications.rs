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
    PersonaDrift,
    SliceBlocker,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopCondition {
    QueueClear,
    QueueStalled,
    StructuralFailure,
    ScopeViolation,
    RetryBudgetExhausted,
    PersonaDrift,
    SliceBlocker,
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

pub fn persona_drift_notification(
    slice_id: &str,
    title: &str,
    author_agent: &str,
    output_length: usize,
) -> NotificationEvent {
    NotificationEvent {
        event: NotificationKind::PersonaDrift,
        title: format!("mutagen — persona drift on {slice_id}"),
        message: format!(
            "Persona drift detected on {slice_id} ({title}): {author_agent} returned {output_length} chars with zero required sections. Capture dispatch payload + transcript before retry."
        ),
    }
}

pub fn slice_blocker_notification(
    slice_id: &str,
    title: &str,
    reason_token: &str,
    body_excerpt: &str,
) -> NotificationEvent {
    let excerpt = body_excerpt.trim();
    let trimmed = if excerpt.chars().count() > 200 {
        let cut: String = excerpt.chars().take(200).collect();
        format!("{cut}…")
    } else {
        excerpt.to_string()
    };
    NotificationEvent {
        event: NotificationKind::SliceBlocker,
        title: format!("mutagen — slice blocker on {slice_id}"),
        message: format!(
            "Slice blocker on {slice_id} ({title}): reason `{reason_token}`. Body: {trimmed}"
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
