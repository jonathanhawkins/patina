---
name: flywheel-worker
description: >-
  Flywheel worker execution skill. Autonomously discovers, claims, and implements
  beads. Use for swarm bead assignments in tmux worker panes.
  Trigger on: "work bead", "flywheel worker",
  "read your inbox for assignment", "send bead-complete", "browser verifier", "regression verifier".
---

# Flywheel Worker

Autonomous worker loop for the agent swarm. You discover your own work, claim it, implement it, and report completion.

## Step 1: Check Your State

Run this script — it checks your beads AND the verifier log and tells you exactly what to do:

```bash
./scripts/flywheel-check.sh
```

Read the ACTION line and follow it:

- **`ACTION: FIX_AND_RESUBMIT`** → The verifier FAILED your bead. Read the failure reason printed below it. Go to **Step 3** to fix the code, then re-report with **Step 5**.
- **`ACTION: WAIT_FOR_VERIFIER`** → Verifier is running. Run the wait command it prints (zero tokens). Then re-run `./scripts/flywheel-check.sh`.
- **`ACTION: WAIT_FOR_CLOSE`** → Verifier passed. Run the sleep command it prints, then go to **Step 1** again.
- **`ACTION: IMPLEMENT_OR_REPORT`** → You have a bead with no verifier activity. If already done, report with **Step 5**. If not, implement with **Step 3**.
- **`ACTION: CLAIM_NEW_WORK`** → No beads assigned. Go to **Step 1b**.

### 1b. Pull new work

**First, guard against hoarding.** Before claiming anything, check whether you already hold an in-progress bead:

```bash
br list --status in_progress --assignee "$AGENT_NAME" --no-auto-import --allow-stale
```

If that returns ANY bead, **Do NOT claim a new bead** — finish (or report/resubmit) the one you already hold and go back to **Step 1**. Workers that claim while holding work hoard beads (one worker once claimed 17 beads without finishing any). Only continue below when you hold zero in-progress beads.

Run this EXACT command. Do NOT pipe it into python. Do NOT add a filter. Do NOT wrap it in any script:

```bash
br ready --json --unassigned --limit 5 --no-auto-import --allow-stale
```

Then extract the first bead's ID and priority — shell only, no filtering logic:

```bash
br ready --json --unassigned --limit 5 --no-auto-import --allow-stale | jq -r '.issues[0] | "\(.id) \(.priority)"'
```

(If `jq` is unavailable, use: `python3 -c 'import json,sys; d=json.load(sys.stdin); i=d["issues"][0] if d["issues"] else None; print(i["id"], i["priority"]) if i else print("")'` — this only prints the first issue, it does NOT filter.)

**Selection rule**: Every bead returned by `br ready --unassigned` IS actionable. Take the FIRST issue from the `issues` array (the list is already priority-ordered: P0 before P1 before P2 before P3). Do NOT inspect labels. Do NOT skip beads based on title content. Do NOT define your own "actionable" predicate.

**Idle ONLY when** the JSON `issues` array is literally empty (`[]`).

### 1c. Claim the bead

```bash
br update <bead-id> --assignee "$AGENT_NAME" --status in_progress --no-auto-import --no-auto-flush
```

Only claim ONE bead.

### 1d. Reserve files (if applicable)

```
/skill mail-reserve <file-paths>
```

## Step 2: Understand the Bead

```bash
br show <bead-id> --no-auto-import --allow-stale
```

Read carefully: what to implement, what files, what tests (look for "Acceptance:" lines).

## Step 3: Implement

Rules:
1. **One bead at a time.**
2. **Follow `AGENTS.md` and `CLAUDE.md`.**
3. **Add or update tests** with the implementation.
4. **DO NOT run `cargo`, `rust_task.sh`, or any Rust compilation.** The verifier is the sole builder. Report your test command in `/mail-complete` and the verifier runs it.
5. For non-Rust checks (docs, scripts, grep, file reads) — run those directly.
6. **If blocked**, report via Agent Mail instead of expanding scope.
7. **Do NOT call `br update --status done` or `br close`** — coordinator handles lifecycle.

## Step 4: Verify (without compiling)

Before reporting completion:
1. Read your code changes and check they look correct
2. Run non-Rust checks: file existence, grep for expected patterns, doc validation
3. Do NOT invoke `cargo` or `rust_task.sh` — the verifier handles that

## Step 5: Report Completion

```bash
cat .beads/coordinator_agent 2>/dev/null
```

Then use the `/mail-complete` skill:

```
/mail-complete <bead-id> --to <coordinator-name> --file <path1> --file <path2> --test "<test-command>"
```

Use `--test` with focused test names like `--test my_specific_test`. Do NOT use `--workspace` or `-E` filter expressions.

**Do NOT send raw MCP** `send_message` calls to report completion. Always use the `/mail-complete` skill — it tags the message with the `bead-complete` topic the coordinator filters on. A raw MCP message has no topic, so the coordinator never processes it and your bead is never verified or closed.

After reporting, go DIRECTLY to **Step 6**. Do NOT say "idle" or "waiting". Do NOT end the iteration.

## Step 6: Wait for Verification (MANDATORY after Step 5)

**You MUST run this bash command immediately after reporting completion.** It costs zero tokens and blocks until the verifier finishes. Do NOT skip this step. Do NOT say "idle" instead.

```bash
BEAD_ID="<your-bead-id>"; echo "Waiting for verifier on $BEAD_ID..."; for i in $(seq 1 20); do RESULT=$(grep "bead=$BEAD_ID" .codex/orchestrator/verifier.log 2>/dev/null | tail -1); if echo "$RESULT" | grep -q "PASS"; then echo "PASS — bead closed"; break; elif echo "$RESULT" | grep -q "FAIL"; then echo "FAIL — $RESULT"; break; fi; sleep 30; done; echo "=== VERIFIER RESULT ==="; grep "bead=$BEAD_ID" .codex/orchestrator/verifier.log 2>/dev/null | tail -3; echo "=== END ==="
```

Read the output:
- **PASS**: Bead closed. Go back to **Step 1** to claim new work.
- **FAIL**: Read the failure reason. Go to **Step 3** to fix the issue, then re-report with **Step 5**.
- **Timeout (no result after 10 min)**: Verifier may be busy. Go back to **Step 1**.

## Hard Rules

- NEVER say "idle" or "waiting for verification" without running the Step 6 bash wait first
- NEVER assume a bead ID from a previous iteration — always read it from `br list` output
- NEVER run `cargo` or `rust_task.sh` — the verifier is the sole Rust builder
- NEVER wrap `br ready` in a python (or any) filter that defines "actionable" beads. Every bead returned by `br ready --unassigned` IS actionable. If `br ready` returns at least one issue, claim it. Only idle when the JSON `issues` array is empty.
- NEVER check `labels`, title substrings, or any other field to decide whether to skip a bead returned by `br ready` — just take `issues[0]`.
- Follow `AGENTS.md`
- Do not create or close beads (coordinator handles lifecycle)
- If blocked, report the block instead of expanding scope
