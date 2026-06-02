//! Session-channel wire DTOs (MD-7 / ISC-010 / ADR-0002).
//!
//! These are *wire* shapes — utoipa schemas live here, never on the domain
//! types from `mutagen_core::session`. Marshalling happens at the WS frame
//! boundary in [`crate::routes::session`].
//!
//! Server→client and client→server frames are JSON text frames with a stable
//! discriminator (`event` for server frames, `op` for client frames). The
//! taxonomy is deliberately narrow at v1; new events get added at minor
//! schema_version bumps (DDD MD-5).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use mutagen_core::session::{Answer, QuestionEnvelope, QuestionKind};

/// FR-11 / ISC-009 wire shape. The `schema_version` field is **required** on
/// the OpenAPI side because the domain constructor stamps it unconditionally.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct QuestionEnvelopeDto {
    /// Chat-protocol schema version. Always set by the server; clients pin
    /// to a known version range (DDD MD-5).
    pub schema_version: String,
    pub question_id: String,
    pub prompt: String,
    /// Question Kind discriminator: `free_text` | `multi_choice` |
    /// `multi_select` | `boolean` | `file_upload` (DDD §3.3 Value Objects).
    pub kind: String,
    /// Kind-specific payload (e.g. `options` for `multi_choice`). Absent for
    /// `free_text` and `boolean`. Schema-less by design — the discriminator
    /// drives client rendering, not this field's shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl From<&QuestionEnvelope> for QuestionEnvelopeDto {
    fn from(e: &QuestionEnvelope) -> Self {
        let payload = match &e.kind {
            QuestionKind::FreeText | QuestionKind::Boolean => None,
            QuestionKind::MultiChoice { options } | QuestionKind::MultiSelect { options } => {
                Some(serde_json::json!({ "options": options }))
            }
            QuestionKind::FileUpload { accept } => Some(serde_json::json!({ "accept": accept })),
        };
        Self {
            schema_version: e.schema_version().to_owned(),
            question_id: e.question_id.clone(),
            prompt: e.prompt.clone(),
            kind: e.kind.name().to_owned(),
            payload,
        }
    }
}

/// Client→server frame.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ClientMessage {
    /// Submit an Answer for an outstanding Question. Mismatched shape yields
    /// `error.validation` (INV-S4) — NOT a state transition.
    SubmitAnswer {
        question_id: String,
        #[schema(value_type = serde_json::Value)]
        answer: Answer,
    },
    /// Test/dev hook so the harness can drive a question→answer round-trip
    /// end-to-end without a live April loop. Hidden behind the
    /// `cfg(any(test, feature = "test-hooks"))` switch on the server side;
    /// the DTO stays here because the OpenAPI surface is one schema.
    IssueQuestion {
        prompt: String,
        kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[schema(value_type = Option<serde_json::Value>)]
        payload: Option<serde_json::Value>,
    },
}

/// Server→client frame. All events carry a snake_case discriminator (DSD-621).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ServerMessage {
    /// April issued a Question Envelope. Echoes the full envelope including
    /// `schema_version` (ISC-009).
    QuestionIssued {
        session_id: String,
        envelope: QuestionEnvelopeDto,
    },
    /// Answer accepted; echoed back so the GUI can clear its pending state
    /// (DDD §3.3 Domain Events `question.answered`).
    QuestionAnswered {
        session_id: String,
        question_id: String,
        answer_kind: String,
    },
    /// POL-S1: timeout fired before an Answer arrived.
    QuestionTimedOut {
        session_id: String,
        question_id: String,
    },
    /// INV-S4 validation failure — kind mismatch or unknown question_id.
    ErrorValidation {
        session_id: String,
        question_id: Option<String>,
        code: String,
        message: String,
    },
    /// L4-Session-003 placeholder — issued when a Workflow Command queues
    /// (POL-S4 first leg). Wired here so the taxonomy is complete in v1's
    /// OpenAPI schema and downstream GUI codegen sees it.
    CommandAccepted {
        session_id: String,
        request_id: String,
        command: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dto_carries_schema_version_for_every_kind() {
        for kind in [
            QuestionKind::FreeText,
            QuestionKind::MultiChoice {
                options: vec!["a".into()],
            },
            QuestionKind::MultiSelect {
                options: vec!["a".into()],
            },
            QuestionKind::Boolean,
            QuestionKind::FileUpload {
                accept: vec!["*/*".into()],
            },
        ] {
            let env = QuestionEnvelope::new(kind, "q");
            let dto: QuestionEnvelopeDto = (&env).into();
            assert!(!dto.schema_version.is_empty());
        }
    }

    #[test]
    fn server_message_event_field_is_snake_case() {
        let msg = ServerMessage::QuestionIssued {
            session_id: "s".into(),
            envelope: (&QuestionEnvelope::new(QuestionKind::FreeText, "q")).into(),
        };
        let s = serde_json::to_string(&msg).unwrap();
        assert!(s.contains("\"event\":\"question_issued\""), "got: {s}");
    }
}
