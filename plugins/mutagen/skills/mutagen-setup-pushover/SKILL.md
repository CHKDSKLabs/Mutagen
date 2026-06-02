---
name: mutagen-setup-pushover
description: Explicit invocation only. First-run Pushover configuration wizard: detect state, collect user key and app token, choose storage (env vars or workflow.json), optionally configure quiet events, send a test notification.
---

# $mutagen-setup-pushover — first-run Pushover configuration wizard

Get Pushover notifications working for `$mutagen-execute-next` halts with as
little friction as possible: detection → credentials → storage → test.
Handle secrets carefully.

## Hard rules on secrets

- **Never echo the user key or app token back.** Acknowledge with a masked
  summary (`user_key: uQi...7xZ` — first three + last three chars).
- **Never save keys to memory, state files, commits, or any file outside the
  storage path the user chose.** Only places keys land: (a) the user's
  shell env / rc file, or (b) `.claude/workflow.json` with an explicit
  don't-commit warning.
- **If the user cancels at any step**, do not persist partial values.

---

## Step 1 — Preflight

1. `mkdir -p .claude`.
2. Detect current state:
   - Env vars `PUSHOVER_USER_KEY` and `PUSHOVER_APP_TOKEN` (both present =
     "env configured").
   - `.claude/workflow.json` `notifications.pushover.user_key` /
     `app_token` / `enabled` (via `jq`).
3. Report masked. If both credentials already resolve, offer: test / re-
   configure / leave it alone. If either is missing, continue without
   asking.

---

## Step 2 — Explain what they're signing up for (skip if veterans)

Briefly (2–3 sentences):

- Pushover is a paid-once ($5 one-time per platform) push service. Free
  7-day trial.
- You need a **user key** from <https://pushover.net/> and an
  **application/API token** from <https://pushover.net/apps/build>.
- Both are 30-char alphanumeric strings.

Ask: *"Do you have both values ready? (yes / no / I need help)"*

---

## Step 3 — Collect credentials

Prompt for user key, then app token, one at a time. Mask after each.
Minimal validation: non-empty, ≥ 20 chars, no whitespace, no shell
metacharacters (`$`, `` ` ``, `;`, newline).

---

## Step 4 — Choose storage

> **1. Environment variables (recommended).** Prints `export` lines. Keys
> never touch the repo.
>
> **2. Project config (`.claude/workflow.json`).** Writes both keys.
> **Do not commit the file.**

Default to 1.

### Path 1 — env vars

1. Print:
   ```bash
   export PUSHOVER_USER_KEY="<key>"
   export PUSHOVER_APP_TOKEN="<token>"
   ```
2. Detect `$SHELL`. Suggest the rc file:
   - `bash` → `~/.bashrc` / `~/.bash_profile`
   - `zsh`  → `~/.zshrc`
   - `fish` → `~/.config/fish/config.fish` (note: `set -x ...`)
3. Ask whether to append. If yes, use heredoc / `printf '%s\n'`.

### Path 2 — workflow.json

1. Warn about not committing. Add `.claude/workflow.json` to `.gitignore`.
2. Merge with `jq`:
   ```bash
   if [ ! -f .claude/workflow.json ]; then echo '{}' > .claude/workflow.json; fi
   jq --arg user "$USER_KEY" --arg token "$APP_TOKEN" \
      '.notifications.pushover = ((.notifications.pushover // {}) + {enabled: true, user_key: $user, app_token: $token})' \
      .claude/workflow.json > .claude/workflow.json.tmp \
     && mv .claude/workflow.json.tmp .claude/workflow.json
   ```
3. Read back and show only the shape with masked values.

---

## Step 5 — Optional: quiet events

Options: `queue_clear`, `structural_fail`, `scope_violation`, `escalation`.
Most users silence nothing. Some silence `queue_clear`.

Path 2: merge into `notifications.pushover.quiet_events`.
Path 1: `quiet_events` needs `.claude/workflow.json`. Offer to write just
that field (safe to commit).

---

## Step 6 — Send a test notification

```bash
PUSHOVER_USER_KEY="<key>" PUSHOVER_APP_TOKEN="<token>" \
  bash "$MUTAGEN_ROOT/scripts/notify.sh" \
    setup_test \
    "mutagen — setup complete" \
    "Push notifications are wired up. You'll get a push like this whenever \$mutagen-execute-next halts at a slice that needs your input."
```

Ask: *"Did the push land? (yes / no)"*. If no: walk back through keys,
token state, device notifications, verbose curl.

---

## Step 7 — Close out

Masked summary. Re-run `$mutagen-setup-pushover` to tweak later.

---

## Reminders

- Conversational, not batch. Prompt → wait → move on.
- If the user is a Pushover veteran, skip Step 2.
- Any cancellation unwinds without persisting.
- `jq` and `curl` are required.
