//! Answer value object (DDD §3.3) and the validation gate that upholds INV-S4
//! — answer shape MUST match the question's Kind, mismatch is a validation
//! error event, not a state transition.
//!
//! Also hosts [`append_user_reply`], the JSONL append helper for FR-13.
//! The wire schema mirrors the elicitation checkpoint format documented in
//! plugins/mutagen/CHANGELOG.md (0.3.3): one record per turn, fields
//! `ts`, `turn`, `mode`, `user_message_summary`, `drafted_paths`,
//! `defaults_filled`, `questions_asked`, `answers_recorded`, `open_tbds`,
//! `consistency_flags`, `readiness_brief_emitted`. We don't reinvent the
//! shape — we *append* — exactly so a service-written reply and a
//! CLI-written reply land as identical lines.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::envelope::{QuestionEnvelope, QuestionKind};

/// Payload supplied by the client in response to a [`QuestionEnvelope`]. The
/// untagged-with-tag pattern (`tag = "kind"`) matches the on-the-wire shape:
/// `{ "kind": "free_text", "text": "..." }` etc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Answer {
    FreeText { text: String },
    MultiChoice { choice: String },
    MultiSelect { choices: Vec<String> },
    Boolean { value: bool },
    FileUpload { paths: Vec<String> },
}

impl Answer {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::FreeText { .. } => "free_text",
            Self::MultiChoice { .. } => "multi_choice",
            Self::MultiSelect { .. } => "multi_select",
            Self::Boolean { .. } => "boolean",
            Self::FileUpload { .. } => "file_upload",
        }
    }

    /// Render the answer back to a human-readable scalar/list for the
    /// `answers_recorded` JSONL field. Keeps the on-disk shape compatible
    /// with what April writes from the CLI plugin path.
    pub fn to_recorded_value(&self) -> serde_json::Value {
        match self {
            Self::FreeText { text } => serde_json::Value::String(text.clone()),
            Self::MultiChoice { choice } => serde_json::Value::String(choice.clone()),
            Self::MultiSelect { choices } => serde_json::json!(choices),
            Self::Boolean { value } => serde_json::Value::Bool(*value),
            Self::FileUpload { paths } => serde_json::json!(paths),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerValidationError {
    pub expected_kind: String,
    pub got_kind: String,
}

impl std::fmt::Display for AnswerValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "answer kind mismatch: expected {}, got {}",
            self.expected_kind, self.got_kind
        )
    }
}

impl std::error::Error for AnswerValidationError {}

/// INV-S4 — the only gate that decides whether an Answer matches a Question.
/// Returns `Err` on shape mismatch (which the caller renders as
/// `error.validation`, not as a state transition).
pub fn validate(envelope: &QuestionEnvelope, answer: &Answer) -> Result<(), AnswerValidationError> {
    let ok = matches!(
        (&envelope.kind, answer),
        (QuestionKind::FreeText, Answer::FreeText { .. })
            | (QuestionKind::MultiChoice { .. }, Answer::MultiChoice { .. })
            | (QuestionKind::MultiSelect { .. }, Answer::MultiSelect { .. })
            | (QuestionKind::Boolean, Answer::Boolean { .. })
            | (QuestionKind::FileUpload { .. }, Answer::FileUpload { .. })
    );
    if !ok {
        return Err(AnswerValidationError {
            expected_kind: envelope.kind.name().to_owned(),
            got_kind: answer.kind_name().to_owned(),
        });
    }
    Ok(())
}

/// FR-13 — append a user reply to `<project_root>/.mutagen/state/elicitation.jsonl`.
/// The schema mirrors what April writes from the CLI plugin path (0.3.3
/// checkpoint format). One line per reply; turn numbers are monotonic and
/// derived from the prior line count so a service-written reply and a
/// CLI-written reply interleave cleanly.
pub fn append_user_reply(
    project_root: &Path,
    envelope: &QuestionEnvelope,
    answer: &Answer,
    user_message_summary: &str,
) -> Result<()> {
    let dir = project_root.join(".mutagen").join("state");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("create elicitation state dir {}", dir.display()))?;
    let path = dir.join("elicitation.jsonl");

    let prior = std::fs::read_to_string(&path).unwrap_or_default();
    let turn = prior.lines().filter(|l| !l.trim().is_empty()).count() + 1;

    let ts = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"));

    let record = serde_json::json!({
        "ts": ts,
        "turn": turn,
        "mode": "iteration",
        "user_message_summary": user_message_summary,
        "drafted_paths": [],
        "defaults_filled": [],
        "questions_asked": [],
        "answers_recorded": [{
            "q": envelope.prompt,
            "question_id": envelope.question_id,
            "kind": envelope.kind.name(),
            "a": answer.to_recorded_value(),
        }],
        "open_tbds": [],
        "consistency_flags": [],
        "readiness_brief_emitted": false,
        "origin": "service",
    });

    let mut line = serde_json::to_string(&record).context("serialize elicitation record")?;
    line.push('\n');

    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    f.write_all(line.as_bytes())
        .with_context(|| format!("append to {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_match_passes() {
        let e = QuestionEnvelope::new(
            QuestionKind::MultiChoice {
                options: vec!["a".into(), "b".into()],
            },
            "pick",
        );
        let a = Answer::MultiChoice { choice: "a".into() };
        assert!(validate(&e, &a).is_ok());
    }

    #[test]
    fn shape_mismatch_yields_validation_error() {
        let e = QuestionEnvelope::new(QuestionKind::Boolean, "yes?");
        let a = Answer::FreeText {
            text: "kinda".into(),
        };
        let err = validate(&e, &a).unwrap_err();
        assert_eq!(err.expected_kind, "boolean");
        assert_eq!(err.got_kind, "free_text");
    }

    #[test]
    fn append_lands_on_disk_in_jsonl_shape() {
        let tmp = tempdir();
        let env = QuestionEnvelope::new(QuestionKind::FreeText, "name?");
        let ans = Answer::FreeText {
            text: "april".into(),
        };
        append_user_reply(&tmp, &env, &ans, "answered name").unwrap();
        let raw = std::fs::read_to_string(tmp.join(".mutagen/state/elicitation.jsonl")).unwrap();
        let v: serde_json::Value = serde_json::from_str(raw.trim()).unwrap();
        assert_eq!(v["turn"], 1);
        assert_eq!(v["answers_recorded"][0]["a"], "april");
        assert_eq!(v["origin"], "service");
    }

    fn tempdir() -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "mutagen-answer-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
