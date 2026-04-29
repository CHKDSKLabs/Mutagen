use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn run_execute_next_drains_a_bounded_parallel_cohort() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.promote_two_slice_cohort();
    workspace.write_text("fake-claude.sh", fake_claude_script_body());
    workspace.init_git_repo();
    let result = run_execute_next_smoke(&workspace, &[]);
    assert_eq!(result["status"], "queue_clear");
    assert_eq!(result["completed_count"], 2);
    assert_eq!(
        result["completion_markers"]
            .as_array()
            .expect("completion_markers should be an array")
            .len(),
        2
    );

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "completed");
    assert_eq!(queue["slices"][1]["status"], "completed");

    let project_state = workspace.read_text("project_state.md");
    let first_marker = project_state
        .find("### L1-orders-001 — 2026-04-23")
        .expect("first state update should be applied");
    let second_marker = project_state
        .find("### L2-orders-002 — 2026-04-23")
        .expect("second state update should be applied");
    assert!(
        first_marker < second_marker,
        "state updates should land in queue order"
    );

    assert!(
        workspace
            .root
            .join("slices/L1-orders-001/summary.md")
            .exists(),
        "first summary should exist"
    );
    assert!(
        workspace
            .root
            .join("slices/L2-orders-002/summary.md")
            .exists(),
        "second summary should exist"
    );
    assert!(
        workspace
            .root
            .join("reviews/L1-orders-001/tiger-claw.md")
            .exists(),
        "first QA report should exist"
    );
    assert!(
        workspace
            .root
            .join("reviews/L2-orders-002/tiger-claw.md")
            .exists(),
        "second QA report should exist"
    );
    assert!(
        !workspace
            .root
            .join(".mutagen/state/active-slice.json")
            .exists(),
        "queue-clear run should not leave an active slice behind"
    );

    let dispatch_log = workspace.read_text(".mutagen/state/dispatch-log.jsonl");
    assert_eq!(dispatch_log.lines().count(), 2);
    assert_eq!(workspace.git_worktree_count(), 1);
    assert!(!workspace.worktree_artifacts_exist());
}

#[test]
fn run_execute_next_stops_after_a_later_cohort_member_escalates() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.promote_two_slice_cohort();
    workspace.write_text("fake-claude.sh", fake_claude_script_body());
    workspace.init_git_repo();

    let result = run_execute_next_smoke(&workspace, &[("FAIL_STRUCTURAL_SLICE", "L2-orders-002")]);
    assert_eq!(result["status"], "escalated");
    assert_eq!(result["completed_count"], 1);
    assert_eq!(result["completed_slices"][0]["slice_id"], "L1-orders-001");
    assert_eq!(result["terminal"]["slice_id"], "L2-orders-002");
    assert_eq!(result["terminal"]["terminal"]["stage"], "structural_check");

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "completed");
    assert_eq!(queue["slices"][1]["status"], "escalated");

    let project_state = workspace.read_text("project_state.md");
    assert!(project_state.contains("### L1-orders-001 — 2026-04-23"));
    assert!(!project_state.contains("### L2-orders-002 — 2026-04-23"));

    assert!(
        workspace
            .root
            .join("slices/L1-orders-001/summary.md")
            .exists(),
        "completed sibling should keep its summary"
    );
    assert!(
        !workspace
            .root
            .join("slices/L2-orders-002/summary.md")
            .exists(),
        "escalated sibling should not finalize a summary"
    );
    assert!(
        workspace
            .root
            .join("reviews/L1-orders-001/tiger-claw.md")
            .exists(),
        "completed sibling should keep its QA report"
    );
    assert!(
        !workspace
            .root
            .join("reviews/L2-orders-002/tiger-claw.md")
            .exists(),
        "structural failure should stop before Tiger Claw runs"
    );
    assert!(
        workspace
            .root
            .join(".mutagen/state/author-output/L2-orders-002.md")
            .exists(),
        "escalated sibling should still import author diagnostics"
    );
    assert!(
        !workspace
            .root
            .join(".mutagen/state/active-slice.json")
            .exists(),
        "escalated cohort run should not leave an active slice behind"
    );

    let dispatch_log = workspace.read_text(".mutagen/state/dispatch-log.jsonl");
    assert_eq!(dispatch_log.lines().count(), 1);
    assert_eq!(workspace.git_worktree_count(), 1);
    assert!(!workspace.worktree_artifacts_exist());
}

#[test]
fn run_execute_next_escalates_on_cohort_merge_conflict() {
    let workspace = FixtureWorkspace::copy("basic_ready");
    workspace.promote_two_slice_cohort();
    workspace.write_text("fake-claude.sh", fake_claude_script_body());
    workspace.init_git_repo();

    let result = run_execute_next_smoke(&workspace, &[("SHARED_QA_CONFLICT", "1")]);
    assert_eq!(result["status"], "escalated");
    assert_eq!(result["completed_count"], 1);
    assert_eq!(result["completed_slices"][0]["slice_id"], "L1-orders-001");
    assert_eq!(result["terminal"]["stage"], "cohort_merge");
    assert_eq!(result["terminal"]["stop_condition"], "merge_conflict");
    assert_eq!(result["terminal"]["slice_id"], "L2-orders-002");
    assert_eq!(
        result["terminal"]["conflicting_path"],
        "tests/qa/shared.qa.test.rs"
    );

    let queue = workspace.read_json("slices/queue.json");
    assert_eq!(queue["slices"][0]["status"], "completed");
    assert_eq!(queue["slices"][1]["status"], "escalated");
    assert!(
        queue["slices"][1]["escalation_reason"]
            .as_str()
            .expect("merge-conflict escalation should include a reason")
            .contains("tests/qa/shared.qa.test.rs")
    );

    let project_state = workspace.read_text("project_state.md");
    assert!(project_state.contains("### L1-orders-001 — 2026-04-23"));
    assert!(!project_state.contains("### L2-orders-002 — 2026-04-23"));

    assert!(
        workspace
            .root
            .join("slices/L1-orders-001/summary.md")
            .exists(),
        "first slice should keep its summary after a cohort merge conflict"
    );
    assert!(
        !workspace
            .root
            .join("slices/L2-orders-002/summary.md")
            .exists(),
        "conflicted slice should not import its finalized summary"
    );
    assert!(
        workspace
            .root
            .join("reviews/L2-orders-002/tiger-claw.md")
            .exists(),
        "conflicted slice should keep its review diagnostics"
    );
    let shared_qa = workspace.read_text("tests/qa/shared.qa.test.rs");
    assert!(
        shared_qa.contains("shared_qa_guard_for_L1_orders_001"),
        "shared QA artifact should remain from the first completed sibling"
    );
    assert!(
        !shared_qa.contains("shared_qa_guard_for_L2_orders_002"),
        "conflicted sibling should not overwrite the shared QA artifact"
    );
    assert!(
        !workspace
            .root
            .join(".mutagen/state/active-slice.json")
            .exists(),
        "merge-conflict halt should not leave an active slice behind"
    );

    let dispatch_log = workspace.read_text(".mutagen/state/dispatch-log.jsonl");
    assert_eq!(dispatch_log.lines().count(), 1);
    assert_eq!(workspace.git_worktree_count(), 1);
    assert!(!workspace.worktree_artifacts_exist());
}

struct FixtureWorkspace {
    root: PathBuf,
}

impl FixtureWorkspace {
    fn copy(name: &str) -> Self {
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name);
        let destination = unique_temp_dir(name);

        copy_dir_recursive(&source, &destination).expect("fixture copy should succeed");

        Self { root: destination }
    }

    fn promote_two_slice_cohort(&self) {
        let mut queue = self.read_json("slices/queue.json");
        queue["slices"][1]["layer"] = json!(1);
        queue["slices"][1]["depends_on"] = json!([]);
        self.write_json("slices/queue.json", &queue);

        let mut workflow = self.read_json(".claude/workflow.json");
        workflow["max_parallel_slices"] = json!(2);
        self.write_json(".claude/workflow.json", &workflow);
    }

    fn init_git_repo(&self) {
        run_git(&self.root, &["init"]);
        run_git(
            &self.root,
            &["config", "user.email", "mutagen@example.test"],
        );
        run_git(&self.root, &["config", "user.name", "Mutagen Harness"]);
        run_git(&self.root, &["add", "."]);
        run_git(&self.root, &["commit", "-m", "fixture", "--quiet"]);
    }

    fn git_worktree_count(&self) -> usize {
        let output = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(&self.root)
            .output()
            .expect("git should be available for smoke tests");

        if !output.status.success() {
            panic!(
                "git worktree list failed:\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count()
    }

    fn read_json(&self, relative_path: &str) -> Value {
        let raw = self.read_text(relative_path);
        serde_json::from_str(&raw).expect("fixture JSON should parse")
    }

    fn write_json(&self, relative_path: &str, value: &Value) {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture write parent should exist");
        }

        let body = serde_json::to_string_pretty(value).expect("fixture JSON should serialize");
        fs::write(path, format!("{body}\n")).expect("fixture JSON should write");
    }

    fn read_text(&self, relative_path: &str) -> String {
        fs::read_to_string(self.root.join(relative_path)).expect("fixture text should read")
    }

    fn write_text(&self, relative_path: &str, body: &str) {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture write parent should exist");
        }

        fs::write(path, body).expect("fixture text should write");
    }

    fn worktree_artifacts_exist(&self) -> bool {
        let root = self.root.join(".mutagen/worktrees");
        if !root.exists() {
            return false;
        }

        fs::read_dir(root)
            .expect("worktree artifact directory should be readable")
            .next()
            .is_some()
    }
}

impl Drop for FixtureWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git should be available for smoke tests");

    if !output.status.success() {
        panic!(
            "git {:?} failed:\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn run_bash(script: &str) {
    let output = Command::new("bash")
        .arg("-lc")
        .arg(script)
        .output()
        .expect("bash should be available for smoke tests");

    if !output.status.success() {
        panic!(
            "bash smoke script failed:\nscript:\n{}\nstdout:\n{}\nstderr:\n{}",
            script,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn run_execute_next_smoke(workspace: &FixtureWorkspace, env_exports: &[(&str, &str)]) -> Value {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("harness crate should live under repo root")
        .to_path_buf();

    let repo_root_wsl = to_wsl_path(&repo_root);
    let workspace_wsl = to_wsl_path(&workspace.root);
    let fake_claude_wsl = to_wsl_path(&workspace.root.join("fake-claude.sh"));
    let queue_validation_wsl = format!("{workspace_wsl}/.mutagen/state/queue-validation.json");
    let result_path = workspace.root.join("run_execute_next.json");
    let env_setup = if env_exports.is_empty() {
        String::new()
    } else {
        env_exports
            .iter()
            .map(|(key, value)| format!("export {key}=\"{value}\""))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let smoke_script = format!(
        r#"
set -euo pipefail
fake_claude="{fake_claude_wsl}"
chmod +x "$fake_claude"
unset FAIL_STRUCTURAL_SLICE SHARED_QA_CONFLICT || true
{env_setup}

cd "{repo_root_wsl}"
mkdir -p "{workspace_wsl}/.mutagen/state"
bash plugins/mutagen/scripts/validate_queue.sh "{workspace_wsl}/slices/queue.json" > "{queue_validation_wsl}"
CLAUDE_BIN="$fake_claude" bash plugins/mutagen/scripts/run_execute_next.sh \
  --workspace-root "{workspace_wsl}" \
  --queue "{workspace_wsl}/slices/queue.json" \
  --queue-validation "{queue_validation_wsl}" \
  --workflow-config "{workspace_wsl}/.claude/workflow.json" \
  --host claude > "{result_wsl}"
"#,
        result_wsl = to_wsl_path(&result_path),
    );

    run_bash(&smoke_script);
    workspace.read_json("run_execute_next.json")
}

fn fake_claude_script_body() -> &'static str {
    r#"#!/usr/bin/env bash
set -euo pipefail

profile=""
prompt=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --print)
      shift
      ;;
    *)
      prompt="$1"
      shift
      ;;
  esac
done

persona="$(printf '%s' "$prompt" | sed -n 's/^# You are //p' | head -n1)"
profile="$(printf '%s' "$persona" | tr '[:upper:]' '[:lower:]')"
slice_id="$(printf '%s' "$prompt" | grep -o 'L[0-9]-[A-Za-z0-9-]\+' | head -n1)"

case "$profile" in
  bebop)
    case "$slice_id" in
      L1-orders-001)
        mkdir -p src/orders tests/orders
        printf '%s\n' \
          'pub fn create_order(id: &str) -> String {' \
          '    format!("order:{id}")' \
          '}' > src/orders/aggregate.rs
        printf '%s\n' \
          '#[test]' \
          'fn creates_an_order_identifier() {' \
          '    assert_eq!("order:123", crate::create_order("123"));' \
          '}' > tests/orders/aggregate_test.rs
        printf '%s\n' \
          '### 🛠️ Execution: L1-orders-001' \
          '#### Intake Report' \
          '- Domain fit: standard execution ✓' \
          '- Traces: [FR-001] ADR-0001 OrderAggregate [ISC-001] [DSD-001]' \
          '#### Code Artifacts' \
          '- `src/orders/aggregate.rs`' \
          '- `tests/orders/aggregate_test.rs`' \
          '#### ISC Upholding Map' \
          '- [ISC-001] `src/orders/aggregate.rs:1` — validates order creation — test: `cargo test`' \
          '#### Verification Artifacts' \
          '- Acceptance: cargo test' \
          '- ISC detection: [ISC-001] contract check for [FR-001]' \
          '- DSD conformance: [DSD-001] naming check with ADR-0001' \
          '#### State Update' \
          '### L1-orders-001 — 2026-04-23' \
          '**Traces:** PRD [FR-001] · ADR [ADR-0001] · DDD [OrderAggregate] · ISC [ISC-001] · DSD [DSD-001]' \
          '**Artifacts:** src/orders/aggregate.rs, tests/orders/aggregate_test.rs' \
          '**Surface:** order aggregate root' \
          '**ISC upholding:**' \
          '- [ISC-001]: src/orders/aggregate.rs:1 — validates order creation — test: `cargo test`' \
          '**Follow-ups:** none'
        ;;
      L2-orders-002)
        mkdir -p src/http tests/http
        printf '%s\n' \
          "pub fn create_order_route() -> &'static str {" \
          '    "201 Created"' \
          '}' > src/http/create_order.rs
        printf '%s\n' \
          '#[test]' \
          'fn responds_with_created() {' \
          '    assert_eq!("201 Created", crate::create_order_route());' \
          '}' > tests/http/create_order_test.rs
        if [[ "${FAIL_STRUCTURAL_SLICE:-}" == "$slice_id" ]]; then
          printf '%s\n' \
            '### 🛠️ Execution: L2-orders-002' \
            '#### Intake Report' \
            '- Domain fit: standard execution ✓' \
            '- Traces: [FR-001] ADR-0001 OrderAggregate [ISC-001] [DSD-001]' \
            '#### Code Artifacts' \
            '- `src/http/create_order.rs`' \
            '- `tests/http/create_order_test.rs`' \
            '#### ISC Upholding Map' \
            '- [ISC-001] `src/http/create_order.rs:1` — preserves order creation contract — test: `cargo test`'
          exit 0
        fi
        printf '%s\n' \
          '### 🛠️ Execution: L2-orders-002' \
          '#### Intake Report' \
          '- Domain fit: standard execution ✓' \
          '- Traces: [FR-001] ADR-0001 OrderAggregate [ISC-001] [DSD-001]' \
          '#### Code Artifacts' \
          '- `src/http/create_order.rs`' \
          '- `tests/http/create_order_test.rs`' \
          '#### ISC Upholding Map' \
          '- [ISC-001] `src/http/create_order.rs:1` — preserves order creation contract — test: `cargo test`' \
          '#### Verification Artifacts' \
          '- Acceptance: cargo test' \
          '- ISC detection: [ISC-001] contract check for [FR-001]' \
          '- DSD conformance: [DSD-001] naming check with ADR-0001' \
          '#### State Update' \
          '### L2-orders-002 — 2026-04-23' \
          '**Traces:** PRD [FR-001] · ADR [ADR-0001] · DDD [OrderAggregate] · ISC [ISC-001] · DSD [DSD-001]' \
          '**Artifacts:** src/http/create_order.rs, tests/http/create_order_test.rs' \
          '**Surface:** order creation HTTP endpoint' \
          '**ISC upholding:**' \
          '- [ISC-001]: src/http/create_order.rs:1 — preserves order creation contract — test: `cargo test`' \
          '**Follow-ups:** none'
        ;;
      *)
        echo "unexpected Bebop slice: $slice_id" >&2
        exit 1
        ;;
    esac
    ;;
  tigerclaw)
    mkdir -p "reviews/$slice_id" "tests/qa"
    printf '%s\n' \
      '#[test]' \
      "fn qa_guard_for_${slice_id//-/_}() {" \
      '    assert!(true);' \
      '}' > "tests/qa/${slice_id}.qa.test.rs"
    if [[ "${SHARED_QA_CONFLICT:-}" == "1" ]]; then
      printf '%s\n' \
        '#[test]' \
        "fn shared_qa_guard_for_${slice_id//-/_}() {" \
        "    assert_eq!(\"$slice_id\", \"$slice_id\");" \
        '}' > "tests/qa/shared.qa.test.rs"
    fi
    printf '%s\n' \
      '#### Verdict' \
      '🟢 Clean' \
      '' \
      '#### Retry Contract' \
      '```json' \
      '{' \
      '  "hatch_eligible": false,' \
      '  "suggested_fix_scope": "none",' \
      '  "suggested_fix_files": [],' \
      '  "suggested_fix_summary": ""' \
      '}' \
      '```' > "reviews/$slice_id/tiger-claw.md"
    cp "reviews/$slice_id/tiger-claw.md" ".mutagen/state/tiger-claw-latest.md"
    printf 'Tiger Claw clean for %s\n' "$slice_id"
    ;;
  *)
    echo "unsupported fake claude persona profile: $profile" >&2
    exit 1
    ;;
esac
"#
}

fn to_wsl_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let bytes = normalized.as_bytes();

    if bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'/' {
        let drive = normalized[0..1].to_ascii_lowercase();
        let rest = normalized[3..].trim_start_matches('/');
        return format!("/mnt/{drive}/{rest}");
    }

    normalized
}

fn unique_temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();

    for attempt in 0..1024 {
        let path = env::temp_dir().join(format!(
            "mutagen-harness-{name}-execute-next-parallel-{}-{nanos}-{attempt}",
            std::process::id()
        ));

        if fs::create_dir(&path).is_ok() {
            return path;
        }
    }

    panic!("failed to allocate a unique temp dir for {name}");
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
        } else {
            if let Some(parent) = destination_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(source_path, destination_path)?;
        }
    }

    Ok(())
}
