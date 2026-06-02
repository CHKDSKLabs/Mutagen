use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

pub const QUEUE_CONTRACT_HASH_BASIS: &str = "execution_contract_v1";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QueueReadinessSnapshot {
    pub queue_path: String,
    pub queue_validation_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_contract_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_contract_hash_basis: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueueReadinessReason {
    QueueJsonMissing,
    QueueValidationOrphaned,
    QueueValidationMissing,
    QueueValidationMalformed,
    QueueValidationStale,
    QueueValidationFailed,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct QueueReadinessFailure {
    pub queue: String,
    pub queue_validation: String,
    pub reason: QueueReadinessReason,
    pub message: String,
    pub issues: Value,
    pub shadow_artifacts: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum QueueReadiness {
    Ready {
        #[serde(flatten)]
        snapshot: QueueReadinessSnapshot,
    },
    NotReady {
        #[serde(flatten)]
        failure: QueueReadinessFailure,
    },
}

pub fn check_queue_readiness(
    queue_path: &Path,
    queue_validation_path: &Path,
) -> Result<QueueReadiness> {
    let existing_shadow_files = [
        queue_path.with_file_name("slicemap.md"),
        queue_path.with_file_name("queue.md"),
    ]
    .into_iter()
    .filter(|path| path.is_file())
    .map(|path| json!(display_path(&path)))
    .collect::<Vec<_>>();

    if !queue_path.is_file() {
        let reason = if queue_validation_path.is_file() {
            QueueReadinessReason::QueueValidationOrphaned
        } else {
            QueueReadinessReason::QueueJsonMissing
        };
        let message = if queue_validation_path.is_file() {
            "Queue validation report is orphaned. The validator report exists but canonical queue JSON is missing. Re-run /mutagen:slice before /mutagen:execute-next."
        } else if !existing_shadow_files.is_empty() {
            "Canonical queue JSON is missing but markdown renderings exist. Re-run /mutagen:slice before /mutagen:execute-next."
        } else {
            "Canonical queue JSON is missing. Re-run /mutagen:slice before /mutagen:execute-next."
        };

        return Ok(not_ready(
            queue_path,
            queue_validation_path,
            reason,
            message,
            json!([]),
            Value::Array(existing_shadow_files),
        ));
    }

    if !queue_validation_path.is_file() {
        return Ok(not_ready(
            queue_path,
            queue_validation_path,
            QueueReadinessReason::QueueValidationMissing,
            "Queue validation report is missing. Re-run /mutagen:slice before /mutagen:execute-next.",
            json!([]),
            json!([]),
        ));
    }

    let raw_validation = fs::read_to_string(queue_validation_path)
        .with_context(|| format!("failed to read {}", display_path(queue_validation_path)))?;
    let validation: Value = match serde_json::from_str(&raw_validation) {
        Ok(value) => value,
        Err(_) => {
            return Ok(not_ready(
                queue_path,
                queue_validation_path,
                QueueReadinessReason::QueueValidationMalformed,
                "Queue validation report is malformed JSON. Re-run /mutagen:slice before /mutagen:execute-next.",
                json!([]),
                json!([]),
            ));
        }
    };

    let report_contract_hash = validation
        .get("queue_contract_hash")
        .and_then(Value::as_str)
        .unwrap_or("");
    let report_contract_basis = validation
        .get("queue_contract_hash_basis")
        .and_then(Value::as_str)
        .unwrap_or("");
    let current_contract_hash =
        if !report_contract_hash.is_empty() && !report_contract_basis.is_empty() {
            queue_contract_hash(queue_path).unwrap_or_default()
        } else {
            String::new()
        };

    if !report_contract_hash.is_empty()
        && !report_contract_basis.is_empty()
        && !current_contract_hash.is_empty()
    {
        if report_contract_basis != QUEUE_CONTRACT_HASH_BASIS
            || report_contract_hash != current_contract_hash
        {
            return Ok(not_ready(
                queue_path,
                queue_validation_path,
                QueueReadinessReason::QueueValidationStale,
                "Queue validation report is stale. The queue execution contract changed after validation. Re-run /mutagen:slice before /mutagen:execute-next.",
                json!([]),
                json!([]),
            ));
        }
    } else if is_mtime_stale(queue_path, queue_validation_path)? {
        return Ok(not_ready(
            queue_path,
            queue_validation_path,
            QueueReadinessReason::QueueValidationStale,
            "Queue validation report is stale. slices/queue.json changed after validation and no contract-hash comparison was available. Re-run /mutagen:slice before /mutagen:execute-next.",
            json!([]),
            json!([]),
        ));
    }

    if validation.get("ok").and_then(Value::as_bool) != Some(true) {
        let issues = validation
            .get("issues")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let validator_message = validation
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("");
        let mut message = "Queue validation report says the queue is not executable. Fix Shredder output and re-run /mutagen:slice before /mutagen:execute-next.".to_string();
        if !validator_message.is_empty() {
            message.push_str(" Validator said: ");
            message.push_str(validator_message);
        }

        return Ok(not_ready(
            queue_path,
            queue_validation_path,
            QueueReadinessReason::QueueValidationFailed,
            &message,
            issues,
            json!([]),
        ));
    }

    Ok(QueueReadiness::Ready {
        snapshot: QueueReadinessSnapshot {
            queue_path: display_path(queue_path),
            queue_validation_path: display_path(queue_validation_path),
            queue_contract_hash: (!current_contract_hash.is_empty())
                .then_some(current_contract_hash),
            queue_contract_hash_basis: (!report_contract_basis.is_empty())
                .then(|| report_contract_basis.to_string()),
        },
    })
}

pub fn require_queue_ready(
    queue_path: &Path,
    queue_validation_path: &Path,
) -> Result<QueueReadinessSnapshot> {
    match check_queue_readiness(queue_path, queue_validation_path)? {
        QueueReadiness::Ready { snapshot } => Ok(snapshot),
        QueueReadiness::NotReady { failure } => {
            bail!("{} ({})", failure.message, reason_name(failure.reason))
        }
    }
}

pub fn readiness_failure_value(failure: &QueueReadinessFailure) -> Result<Value> {
    serde_json::to_value(failure).context("failed to serialize queue readiness failure")
}

pub fn reason_name(reason: QueueReadinessReason) -> &'static str {
    match reason {
        QueueReadinessReason::QueueJsonMissing => "queue_json_missing",
        QueueReadinessReason::QueueValidationOrphaned => "queue_validation_orphaned",
        QueueReadinessReason::QueueValidationMissing => "queue_validation_missing",
        QueueReadinessReason::QueueValidationMalformed => "queue_validation_malformed",
        QueueReadinessReason::QueueValidationStale => "queue_validation_stale",
        QueueReadinessReason::QueueValidationFailed => "queue_validation_failed",
    }
}

pub fn queue_contract_hash(queue_path: &Path) -> Result<String> {
    let raw = fs::read_to_string(queue_path)
        .with_context(|| format!("failed to read {}", display_path(queue_path)))?;
    let queue: Value = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", display_path(queue_path)))?;
    let contract = queue_contract_value(&queue);
    let contract_json = serde_json::to_string(&contract)?;

    Ok(sha1_hex(contract_json.as_bytes()))
}

fn not_ready(
    queue_path: &Path,
    queue_validation_path: &Path,
    reason: QueueReadinessReason,
    message: &str,
    issues: Value,
    shadow_artifacts: Value,
) -> QueueReadiness {
    QueueReadiness::NotReady {
        failure: QueueReadinessFailure {
            queue: display_path(queue_path),
            queue_validation: display_path(queue_validation_path),
            reason,
            message: message.to_string(),
            issues,
            shadow_artifacts,
        },
    }
}

fn is_mtime_stale(queue_path: &Path, queue_validation_path: &Path) -> Result<bool> {
    let queue_mtime = modified_at(queue_path)?;
    let validation_mtime = modified_at(queue_validation_path)?;
    Ok(queue_mtime > validation_mtime)
}

fn modified_at(path: &Path) -> Result<SystemTime> {
    fs::metadata(path)
        .with_context(|| format!("failed to stat {}", display_path(path)))?
        .modified()
        .with_context(|| format!("failed to read mtime for {}", display_path(path)))
}

fn queue_contract_value(queue: &Value) -> Value {
    let slices = queue
        .get("slices")
        .and_then(Value::as_array)
        .map(|slices| {
            slices
                .iter()
                .map(|slice| {
                    json!({
                        "id": value_or_null(slice, "id"),
                        "title": value_or_null(slice, "title"),
                        "phase": value_or_null(slice, "phase"),
                        "author_agent": value_or_null(slice, "author_agent"),
                        "layer": value_or_null(slice, "layer"),
                        "bounded_context": value_or_null(slice, "bounded_context"),
                        "target_loc": value_or_null(slice, "target_loc"),
                        "objective": value_or_null(slice, "objective"),
                        "context_to_update": value_or_null(slice, "context_to_update"),
                        "implementation_details": value_or_array(slice, "implementation_details"),
                        "review_required": value_or_null(slice, "review_required"),
                        "depends_on": value_or_array(slice, "depends_on"),
                        "adjacent_scope_allowed": value_or_array(slice, "adjacent_scope_allowed"),
                        "write_set": value_or_array(slice, "write_set"),
                        "traces_to": {
                            "prd": pointer_or_array(slice, "/traces_to/prd"),
                            "adr": pointer_or_array(slice, "/traces_to/adr"),
                            "ddd": pointer_or_array(slice, "/traces_to/ddd"),
                            "isc": pointer_or_array(slice, "/traces_to/isc"),
                            "dsd": pointer_or_array(slice, "/traces_to/dsd"),
                        },
                        "verification_steps": {
                            "acceptance": pointer_or_string(slice, "/verification_steps/acceptance"),
                            "isc_detection": pointer_or_string(slice, "/verification_steps/isc_detection"),
                            "dsd_conformance": pointer_or_string(slice, "/verification_steps/dsd_conformance"),
                        },
                        "human_check_needed": {
                            "required": pointer_or_bool(slice, "/human_check_needed/required"),
                            "reason": pointer_or_string(slice, "/human_check_needed/reason"),
                            "resolved_at": slice.pointer("/human_check_needed/resolved_at").cloned().unwrap_or(Value::Null),
                        },
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let planning_advisories = queue
        .get("planning_advisories")
        .and_then(Value::as_array)
        .map(|advisories| {
            advisories
                .iter()
                .map(|advisory| {
                    json!({
                        "id": value_or_null(advisory, "id"),
                        "severity": value_or_null(advisory, "severity"),
                        "summary": value_or_null(advisory, "summary"),
                        "decision": value_or_null(advisory, "decision"),
                        "user_response_required": value_or_null(advisory, "user_response_required"),
                        "references": value_or_array(advisory, "references"),
                        "affects_slices": value_or_array(advisory, "affects_slices"),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    json!({
        "version": value_or_null(queue, "version"),
        "generated_at": value_or_null(queue, "generated_at"),
        "generated_by": value_or_null(queue, "generated_by"),
        "pipeline_mode": value_or_null(queue, "pipeline_mode"),
        "planning_advisories": planning_advisories,
        "slices": slices,
    })
}

fn value_or_null(value: &Value, key: &str) -> Value {
    value.get(key).cloned().unwrap_or(Value::Null)
}

fn value_or_array(value: &Value, key: &str) -> Value {
    value.get(key).cloned().unwrap_or_else(|| json!([]))
}

fn pointer_or_array(value: &Value, pointer: &str) -> Value {
    value.pointer(pointer).cloned().unwrap_or_else(|| json!([]))
}

fn pointer_or_string(value: &Value, pointer: &str) -> Value {
    value.pointer(pointer).cloned().unwrap_or_else(|| json!(""))
}

fn pointer_or_bool(value: &Value, pointer: &str) -> Value {
    value
        .pointer(pointer)
        .cloned()
        .unwrap_or(Value::Bool(false))
}

fn sha1_hex(input: &[u8]) -> String {
    let mut bytes = input.to_vec();
    let bit_len = (bytes.len() as u64) * 8;

    bytes.push(0x80);
    while bytes.len() % 64 != 56 {
        bytes.push(0);
    }
    bytes.extend_from_slice(&bit_len.to_be_bytes());

    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xefcd_ab89;
    let mut h2: u32 = 0x98ba_dcfe;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xc3d2_e1f0;

    for chunk in bytes.chunks_exact(64) {
        let mut words = [0u32; 80];
        for (index, word) in words.iter_mut().take(16).enumerate() {
            let offset = index * 4;
            *word = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }

        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }

        let mut a = h0;
        let mut b = h1;
        let mut c = h2;
        let mut d = h3;
        let mut e = h4;

        for (index, word) in words.iter().enumerate() {
            let (f, k) = match index {
                0..=19 => ((b & c) | ((!b) & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    format!("{h0:08x}{h1:08x}{h2:08x}{h3:08x}{h4:08x}")
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
