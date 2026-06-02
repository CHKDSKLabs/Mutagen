//! Question Envelope (DDD §3.3 aggregate root). The single most consumer-visible
//! wire shape in the service — every multi-choice button in the downstream GUI
//! lands here — so the construction discipline is loud: `schema_version` is
//! private and the only way to set it is `QuestionEnvelope::new`, which copies
//! the compile-time [`CHAT_PROTOCOL_VERSION`] in. ISC-009 falls out of the type
//! definition: an envelope built without a version simply does not compile.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::CHAT_PROTOCOL_VERSION;

/// FR-11 v1 taxonomy. Future Kinds get added here; clients that don't
/// recognise a Kind degrade gracefully by inspecting `schema_version`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuestionKind {
    FreeText,
    MultiChoice { options: Vec<String> },
    MultiSelect { options: Vec<String> },
    Boolean,
    FileUpload { accept: Vec<String> },
}

impl QuestionKind {
    /// Stable wire name. Lives on `QuestionEnvelope` for DSD-621 snake_case
    /// fidelity and for the answer-shape gate (INV-S4) to discriminate without
    /// re-serialising.
    pub fn name(&self) -> &'static str {
        match self {
            Self::FreeText => "free_text",
            Self::MultiChoice { .. } => "multi_choice",
            Self::MultiSelect { .. } => "multi_select",
            Self::Boolean => "boolean",
            Self::FileUpload { .. } => "file_upload",
        }
    }
}

/// Question Envelope. `schema_version` is **private** — see ISC-009. The only
/// construction path is [`QuestionEnvelope::new`], which stamps the version
/// from the compile-time constant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionEnvelope {
    schema_version: String,
    pub question_id: String,
    pub prompt: String,
    #[serde(flatten)]
    pub kind: QuestionKind,
}

impl QuestionEnvelope {
    pub fn new(kind: QuestionKind, prompt: impl Into<String>) -> Self {
        Self {
            schema_version: CHAT_PROTOCOL_VERSION.to_owned(),
            question_id: Uuid::now_v7().to_string(),
            prompt: prompt.into(),
            kind,
        }
    }

    /// Test/replay shim — rebuild an envelope whose question_id matches an
    /// existing record (e.g. when resuming from elicitation.jsonl). The
    /// version still comes from the compile-time constant; reconstructing
    /// from a stored id does not let a caller smuggle in a stale version.
    pub fn with_id(
        question_id: impl Into<String>,
        kind: QuestionKind,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: CHAT_PROTOCOL_VERSION.to_owned(),
            question_id: question_id.into(),
            prompt: prompt.into(),
            kind,
        }
    }

    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_carries_schema_version() {
        let e = QuestionEnvelope::new(QuestionKind::FreeText, "what's your name");
        assert_eq!(e.schema_version(), CHAT_PROTOCOL_VERSION);
        assert!(!e.question_id.is_empty());
    }

    #[test]
    fn schema_version_round_trips_through_serde() {
        let e = QuestionEnvelope::new(
            QuestionKind::MultiChoice {
                options: vec!["yes".into(), "no".into()],
            },
            "pick one",
        );
        let s = serde_json::to_string(&e).unwrap();
        assert!(s.contains("\"schema_version\":\""));
        let back: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(back["schema_version"], CHAT_PROTOCOL_VERSION);
        assert_eq!(back["kind"], "multi_choice");
    }

    #[test]
    fn kind_name_is_snake_case() {
        assert_eq!(QuestionKind::FreeText.name(), "free_text");
        assert_eq!(
            QuestionKind::MultiChoice { options: vec![] }.name(),
            "multi_choice"
        );
        assert_eq!(
            QuestionKind::FileUpload { accept: vec![] }.name(),
            "file_upload"
        );
    }
}
