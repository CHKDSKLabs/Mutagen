---
description: Walk the user through first-run Pushover configuration — detect current state, collect their user key and app token, choose a storage path (env vars or workflow.json), optionally configure quiet events, and send a test notification.
---

# Setup-pushover — first-run Pushover configuration wizard

The user has invoked `/mutagen:setup-pushover`. Your job is to get Pushover notifications working for `/mutagen:execute-next` halts with as little friction as possible. You walk them through detection → credentials → storage → test, and you handle secrets carefully.

## Hard rules on secrets

- **Never echo the user key or app token back to the user.** After they provide a value, acknowledge with a masked summary (`user_key: uQi...7xZ` showing first three + last three). They already know the value — re-printing it in chat accomplishes nothing and leaves it in more scrollback.
- **Never save keys to memory, state files, commits, or any file outside the storage path the user chooses.** Don't write them to `.claude/state/**`, don't include them in a task description, don't paste them into the Pushover section of `workflow.json` *and* also tell the user to commit the file. The only places keys land are (a) the user's shell env / rc file, or (b) `.claude/workflow.json` with an explicit don't-commit warning.
- **If the user cancels at any step**, do not persist partial values. Leave the environment as you found it.

---

## Step 1 — Preflight

1. `mkdir -p .claude`.
2. Detect current state by checking, in this order:
   - Env vars `PUSHOVER_USER_KEY` and `PUSHOVER_APP_TOKEN` (both present counts as "env configured").
   - `.claude/workflow.json` `notifications.pushover.user_key` / `app_token` / `enabled` (via `jq`, if `jq` is on PATH and the file exists).
3. Report what you found to the user, using masking (first three + last three chars, or `(missing)`):
   - `Env vars: PUSHOVER_USER_KEY = uQi...7xZ, PUSHOVER_APP_TOKEN = (missing)`
   - `workflow.json: enabled=true, user_key=aaa...zzz, app_token=(missing), quiet_events=[]`
4. If **both** credentials already resolve from some combination of the two sources, tell the user the plugin is already configured. Offer three next steps:
   - **Send a test notification** (skip to Step 6 with the existing values).
   - **Reconfigure** (continue with Step 2).
   - **Leave it alone** (exit).

   If either credential is missing, continue with Step 2 without asking.

---

## Step 2 — Explain what they're signing up for (skip if already Pushover users)

Briefly describe, in two or three sentences at most:

- Pushover is a paid-once ($5, one-time per platform) push service. Free 7-day trial.
- You need two values: a **user key** (from <https://pushover.net/> after sign-up, shown on the dashboard) and an **application/API token** (create a dedicated application at <https://pushover.net/apps/build> — name it `mutagen` or whatever you want; the API token appears on the app page after creation).
- Both values are 30-char alphanumeric strings.

Ask: *"Do you have both values ready? (yes / no / I need help)"*

- If `no` or `help`: give them the two URLs, wait for them to come back with both values.
- If `yes`: continue.

---

## Step 3 — Collect credentials

Prompt for the user key, then the app token, one at a time. After each, immediately mask the echo (`Got it — user_key: uQi...7xZ`). Don't validate the format beyond "non-empty, ≥ 20 chars" — Pushover's accepted length varies and you don't want false-reject on a valid key.

If either value looks obviously wrong (empty, contains whitespace, contains shell metacharacters like `$`, `` ` ``, `;`, or newlines): ask the user to paste again. Those chars don't appear in real Pushover tokens and would break shell quoting.

---

## Step 4 — Choose storage

Present two paths with a clear recommendation:

> **1. Environment variables (recommended).** Prints `export` lines you paste into your shell or append to your shell rc file. Keys never touch the repo. This is the standard pattern for secrets.
>
> **2. Project config (`.claude/workflow.json`).** Writes `enabled: true` + both keys into `notifications.pushover.{...}`. Simpler if you only ever run the plugin from one machine, but you **must not commit the file** — the warning will remind you.

Ask which they want. Default to 1 if they hesitate.

### Path 1 — env vars

1. Build the two export lines:

   ```bash
   export PUSHOVER_USER_KEY="<key>"
   export PUSHOVER_APP_TOKEN="<token>"
   ```

2. Print them to the user verbatim (this *does* show the keys, but it's the only way to hand them over — the user is the only intended recipient).
3. Detect `$SHELL` via `echo "$SHELL"`. Suggest the appropriate rc file:
   - `/bin/bash` or `bash` → `~/.bashrc` (Linux) / `~/.bash_profile` (macOS) / `~/.bashrc` (Git Bash on Windows)
   - `/bin/zsh` or `zsh` → `~/.zshrc`
   - `fish` → `~/.config/fish/config.fish` and note that fish uses `set -x PUSHOVER_USER_KEY "..."` syntax, not `export`.
   - Anything else → tell the user to add the exports wherever their shell loads env on startup.
4. Ask: *"Want me to append these to `<detected-rc-file>`? (yes / no / different path)"*
   - If `yes`: append. Quote the key values safely using a heredoc or `printf '%s\n'`. Don't `echo` with unescaped interpolation.
   - If `no`: remind the user to paste the lines somewhere the shell will pick them up.
   - If `different path`: use that path after confirming it exists or asking to create it.
5. Also set the two vars in the current shell for the Step 6 test: we can't make `export` stick across the orchestrator's Bash calls in the same session, so for the test we pass the values inline (see Step 6).

### Path 2 — workflow.json

1. Warn verbatim: *"`.claude/workflow.json` will contain the keys in plaintext. Do not commit this file. I'll add it to `.gitignore` if it isn't already."*
2. Check if `.claude/workflow.json` is ignored. If not, offer to add `.claude/workflow.json` to the repo's `.gitignore`. Create `.gitignore` if it's missing.
3. Merge the new section into the file using `jq`. If the file doesn't exist yet, start from `{}`:

   ```bash
   if [ ! -f .claude/workflow.json ]; then echo '{}' > .claude/workflow.json; fi
   jq --arg user "$USER_KEY" --arg token "$APP_TOKEN" \
      '.notifications.pushover = ((.notifications.pushover // {}) + {enabled: true, user_key: $user, app_token: $token})' \
      .claude/workflow.json > .claude/workflow.json.tmp \
     && mv .claude/workflow.json.tmp .claude/workflow.json
   ```

   Pass `$USER_KEY` and `$APP_TOKEN` via `--arg` so jq quotes them correctly regardless of content.
4. Confirm by reading back the file and showing only the shape — `notifications.pushover: { enabled: true, user_key: <masked>, app_token: <masked> }`.

---

## Step 5 — Optional: configure quiet events

Ask once: *"Want to silence any event types? Options: `queue_clear` (the good-news ping when the queue runs dry), `structural_fail`, `scope_violation`, `escalation`. Press enter for none."*

Most users silence nothing. Some silence `queue_clear`. If they name any:

- If they chose Path 2 (workflow.json): merge into `notifications.pushover.quiet_events` with jq.
- If they chose Path 1 (env vars): `quiet_events` isn't supported via env — tell them they'd need to add it to `.claude/workflow.json` separately. Offer to write just that field (no keys) into the file, which is safe to commit.

---

## Step 6 — Send a test notification

Fire `notify.sh` with a `setup_test` event (defaults to normal priority) so the user sees a real push land on their device:

```bash
PUSHOVER_USER_KEY="<key>" PUSHOVER_APP_TOKEN="<token>" \
  bash "${CLAUDE_PLUGIN_ROOT}/scripts/notify.sh" \
    setup_test \
    "mutagen — setup complete" \
    "Push notifications are wired up. You'll get a push like this whenever /mutagen:execute-next halts at a slice that needs your input."
```

Pass the keys inline so the test works regardless of whether the user chose env vars (may not be exported in this shell yet) or workflow.json (already on disk, but inline is fine either way).

Ask the user: *"Did the push land on your device? (yes / no)"*

- `yes`: wrap up (Step 7).
- `no`: walk back through:
  1. Was the user key copied correctly? (Ask them to re-verify.)
  2. Is the app token from an active Pushover application (not deleted, not rate-limited)?
  3. Does the device have notifications enabled for the Pushover app?
  4. Did `curl` succeed? Re-run with verbose output (`curl -v ... 2>&1 | tail -20`) and look for HTTP 4xx responses from api.pushover.net — 400 typically means bad credentials; 429 means rate-limited.
  
  Offer to retry the test once they've checked.

---

## Step 7 — Close out

Summarise what got configured, masking keys:

> **Pushover configured.**
> - Storage: env vars / `.claude/workflow.json`
> - User key: `uQi...7xZ`
> - App token: `azG...3kR`
> - Quiet events: `queue_clear` (or `none`)
> - Test notification: ✓
>
> The next `/mutagen:execute-next` run will push high-priority alerts on structural fails, scope violations, and retry-budget exhaustion. Normal-priority ping on queue clear unless silenced.
> Tweak anything later by editing `.claude/workflow.json` or unsetting the env vars, or re-run `/mutagen:setup-pushover`.

---

## Reminders

- This command is **conversational**, not batch. Don't try to collect everything up front. Prompt → wait → move on.
- If the user is already a Pushover veteran ("I know what I'm doing, just take my keys"), skip Step 2 and the shell-rc explanation; drop straight to Step 3.
- Any step can be cancelled — if the user says "stop" or "nevermind" at any point, unwind without persisting partial config.
- Do not run this command silently from another command. `/mutagen:execute-next` and friends call `notify.sh` directly and let it no-op when unconfigured; they don't launch this wizard.
- `jq` and `curl` are required. If either is missing, say so up front and stop — the plugin's other commands already require `jq`, so this is rarely a surprise.

$ARGUMENTS
