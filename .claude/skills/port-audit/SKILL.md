---
name: port-audit
description: Full-engine fresh-eyes 1:1 Godot→Patina port audit. Sweeps EVERY subsystem (graphics, VFX, audio, animation, physics, multiplayer, GUI, input, resources, editor UX, web runtime) against Godot 4.6.1 ground truth, adversarially verifies findings, and files implementation beads for every gap. Run after completing a large batch of work.
argument-hint: [subsystem-keys… | --dry-run]
trigger: "port audit", "parity audit", "audit the port", "1:1 audit", "fresh eyes audit", "full engine audit", "evaluate the port"
---

# Port Audit — fresh-eyes 1:1 Godot parity sweep

Adversarial, full-surface audit of Patina against Godot 4.6.1. The output is not a
report — it is **implementation beads** for every verified gap, plus a summary the
operator can trust. The mandate: no stone unturned, from rendering and VFX to
multiplayer, audio, animation, GUI, the editor, and the wasm target itself.

This is deliberately expensive: expect **60–120 subagents and 30–90 minutes** on a full
run. Do not downscale coverage to save tokens — the user opted into this by invoking
the skill. Scoped runs (`/port-audit audio animation`) exist for cheap spot re-audits.

## The Three Laws

1. **Fresh eyes, every run.** Patina's own documentation is a set of claims under
   test, never evidence. Nobody in this run reads COMPAT_MATRIX.md, dashboards,
   docs/audits/, prior epics, planner logs, or parity reports to decide what is done.
   The feature checklist is derived from Godot itself (extension API dump +
   `upstream/godot/` source + doc XML) freshly each run. Never limit the audit to
   "what changed recently" — regressions and false closes are exactly what this
   catches. Prior-run artifacts are consulted ONLY by the mechanical `br search`
   dedup in Phase 3.
2. **Wired, not written.** Code existing ≠ parity. The systemic failure mode in this
   codebase is real, tested library code that nothing ever calls (3D physics, audio,
   navigation, AnimationTree, and editor JS have all shipped this way). The Parity
   Ladder a feature must fully climb: **(a)** API surface registered (ClassDB names/
   defaults match Godot), **(b)** behavior implemented (no stubs/default returns),
   **(c)** wired — a call path from the runner frame loop / SceneTree tick / editor
   server / served EDITOR_HTML actually reaches it, **(d)** app-level test proves the
   wiring, **(e)** for visual features, renders equivalently. Findings name the
   missing rungs; module-level green tests prove nothing above rung (b).
3. **Beads are implementation, never meta.** Every bead is a concrete code change
   with real files, an approach, and non-null acceptance criteria naming a scoped
   test — the ``(test: `name`)`` convention, runnable as `--test <name>`. Never
   create "audit/investigate/align X" beads. The audit itself is the investigation.

## Phase 0 — Prep (inline, ~2 min)

Set up the run directory and oracles. Use the session scratchpad as `$RUN_BASE`.

```bash
RUN_DATE=$(date +%F)
RUN_DIR="$RUN_BASE/port-audit-$RUN_DATE"   # $RUN_BASE = session scratchpad dir
mkdir -p "$RUN_DIR/beads"
ROOT=/Users/bone/dev/games/patina
GODOT=/Applications/Godot.app/Contents/MacOS/Godot

# 1. Dump the machine-readable Godot surface (fresh each run — tracks installed Godot)
(cd "$RUN_DIR" && "$GODOT" --headless --dump-extension-api >/dev/null 2>&1)
GODOT_VERSION=$(jq -r '.header.version_full_name' "$RUN_DIR/extension_api.json")
jq -r '.classes[].name' "$RUN_DIR/extension_api.json" | sort > "$RUN_DIR/godot_classes.txt"

# 2. Snapshot Patina's registered classes (adapt the grep if class_db syntax changed)
grep -rhoE '"[A-Z][A-Za-z0-9]+"' "$ROOT/engine-rs/crates/gdobject/src/class_db.rs" \
  | tr -d '"' | sort -u > "$RUN_DIR/patina_registered.txt"

# 3. The sweep list: Godot classes with no Patina registration
comm -23 "$RUN_DIR/godot_classes.txt" "$RUN_DIR/patina_registered.txt" > "$RUN_DIR/unregistered.txt"
wc -l "$RUN_DIR"/godot_classes.txt "$RUN_DIR"/patina_registered.txt "$RUN_DIR"/unregistered.txt
```

Sanity checks before proceeding:
- `GODOT_VERSION` should be 4.6.1-stable — record it; if the binary is missing, fall
  back to `ls $ROOT/upstream/godot/doc/classes | sed 's/\.xml$//' | grep -v '^@'` as
  the class list and note the degraded oracle in the report.
- `patina_registered.txt` should have well over 100 names. If it looks tiny or noisy,
  open `class_db.rs`, adapt the grep to the actual registration syntax, and also
  grep other crates for `register_class` call sites — do not proceed with a bad diff.
- Snapshot queue health for the report:
  `br count --by-status --json --no-auto-import --allow-stale`

Parse `$ARGUMENTS`: tokens matching taxonomy keys scope the run to those subsystems
(and **skip Sweep + Critic**); `--dry-run` means run everything but create no beads.

## Phase 1 — The workflow (finders → verify, sweep, critic)

The subsystem keys (must match `references/taxonomy.md` sections):

```
core-object core-variant gdscript scene-tree nodes-2d rendering-2d nodes-3d
rendering-3d shaders vfx-particles animation physics-2d physics-3d navigation
audio multiplayer gui-controls theming-text input resources-io platform-os
editor-ux web-runtime xr-peripheral
```

Read `$RUN_DIR/unregistered.txt` and chunk it into arrays of ~50 names. Then invoke
the Workflow tool (this skill is your authorization to use it):

```
Workflow({
  scriptPath: "<ROOT>/.claude/skills/port-audit/references/workflow.js",
  args: {
    runDate: "<RUN_DATE>", godotVersion: "<GODOT_VERSION>",
    root: "<ROOT>", runDir: "<RUN_DIR>",
    api: "<RUN_DIR>/extension_api.json",
    taxonomy: "<ROOT>/.claude/skills/port-audit/references/taxonomy.md",
    subsystems: [ ...the 24 keys, or the scoped subset... ],
    sweepChunks: [ [..50 names..], ... ]   // [] on scoped runs
    // skipCritic: true on scoped runs
  }
})
```

While it runs, do nothing destructive; it is read-only by design. When it completes:
- If `dropped` is non-empty, re-run those subsystems (fix whatever broke, then use
  `resumeFromRunId` or a scoped follow-up call) **before** synthesizing. A dropped
  subsystem is a silent hole — forbidden.
- On anomalies (empty results, weird counts), Read the run's `journal.jsonl` in the
  workflow transcript dir before drawing conclusions.

## Phase 2 — Synthesis (inline)

Work from the workflow's return value (`subsystems`, `sweep`, `supplemental`):

1. Apply verdicts: drop `REFUTED` findings; apply `ADJUSTED` corrections
   (status/priority). Findings with no verdict (verifier chunk died) are kept but
   marked `unverified: true` — cap them at P1 and say so in their bead description.
2. Merge duplicates **mechanically**: two findings collide when they share a primary
   entry in `godot_classes` or an identical normalized `group_key`. Union evidence,
   keep max priority, prefer the finder version over the sweep version.
3. Group into bead drafts by `group_key` (sweep keys are `<taxonomy-key>/<slug>` —
   the prefix is the subsystem label). One bead ≈ one worker session.
4. Collect `OUT_OF_SCOPE` findings separately — they get NO beads; they go in the
   report for human ratification with their citations.
5. Write each bead body to `$RUN_DIR/beads/<slug>.md` (see template below).

Bead description template (`$RUN_DIR/beads/<slug>.md`):

```
[audit-key: <subsystem>/<group_key>] [port-audit <RUN_DATE>] [status: <MISSING|STUB|UNWIRED|PARTIAL>]

**Godot ground truth:** <behavior + upstream/godot ref or api-json pointer>
**Patina today:** <file:line evidence; searches that came up empty>
**Implement:** <concrete approach naming real files>
**Wiring requirement:** <the end-to-end call path that must exist when done>
**Prove it:** <the app-level test — what it loads/drives and what it asserts>
```

## Phase 3 — Beads (inline, serial `br` — the only writer is you)

Skip entirely on `--dry-run` (still write the report, marked DRY RUN).

```bash
br sync --rebuild 2>&1 | tail -1
EPIC=$(br create "Port audit $RUN_DATE: 1:1 Godot $GODOT_VERSION parity gaps" \
  -t epic -p 1 -l port-audit,fresh-audit \
  -d "Fresh-eyes full-surface audit (skill: port-audit). Oracle: extension_api $GODOT_VERSION + upstream/godot source. N subsystems, M findings after adversarial verify. Children carry [audit-key:] markers." \
  --json | jq -r '.id')
```

Then for every bead draft, **P0s first**:

```bash
# Dedup — EXACT command and rule, no exceptions:
N=$(br search "$TITLE" --status open --status in_progress --json --no-auto-import --allow-stale | jq -r 'length')
```

- `N >= 1` → SKIP creation. Do NOT inspect titles or judge "close enough" — any hit
  is a duplicate. Record the matched ids (`| jq -r '.[].id'`) under "already tracked"
  in the report. If the finding is P0, add a comment to the first match:
  `br comments add "$DUP_ID" "port-audit $RUN_DATE re-confirmed (P0): <one-line evidence>"`.
  (Deliberate deviation from /editor-parity: CLOSED beads do **not** block creation —
  the finding is code-verified against the current tree, so a closed bead claiming
  otherwise is exactly the false-done signal this audit exists to catch.)
- `N == 0` → create:

```bash
ID=$(br create "$TITLE" -t "$TYPE" -p "$PRIO" -l "port-audit,subsys:$KEY" \
  --parent "$EPIC" -d "$(cat "$RUN_DIR/beads/$SLUG.md")" --json | jq -r '.id')
br update "$ID" --acceptance-criteria "$CRITERIA"
```

`$TYPE`: `bug` for UNWIRED/STUB (false-done), `feature` for MISSING/PARTIAL.
`$PRIO`: the verified 0–3 priority. `$CRITERIA` must be the observable app-level
behavior and end with `(test: \`snake_case_name\`)`.

For a large batch, creating with `--no-auto-flush` (on both `br create` and
`br update`) is much faster, but it leaves the new beads **DB-only** — a plain
`br sync` will NOT export them (it reports "JSONL is current"). You MUST finish with
an explicit export or the beads never reach `.beads/issues.jsonl` (what git and other
agents read):

```bash
br sync --flush-only 2>&1 | tail -2   # DB -> JSONL; confirms "Exported: N issues"
```

Verify the export landed: `grep -c '<epic-id>' .beads/issues.jsonl` should equal
children+1. (If you let each `br create` auto-flush instead, skip this — but it is
markedly slower over hundreds of beads.)

Post-create verification (abort and report if any fails):
- The first created bead and EVERY P0 bead:
  `br show "$ID" --json | jq '.[0] | {description, acceptance_criteria}'` — both
  fields non-null, no template placeholders, `[audit-key:` present.

## Phase 4 — Report

Write `docs/audits/port-audit-$RUN_DATE.md` (create the dir if needed; do not commit;
future audit runs are forbidden from reading it as evidence — it is for humans):

```
# Port Audit — <RUN_DATE> (oracle: Godot <GODOT_VERSION>)
Epic: <EPIC> · Findings: <n> verified (<r> refuted, <u> unverified) · Beads: <c> created, <s> already tracked
## Per-subsystem
| Subsystem | Parity-OK | MISSING | STUB | UNWIRED | PARTIAL | P0s | Coverage confession |
## P0 list (bead ids + one-liners)
## Proposed OUT_OF_SCOPE (needs human ratification — citations included)
## Dropped/degraded (subsystems re-run, oracle fallbacks — "none" expected)
## Critic notes
```

Then give the user the same thing in chat, compressed: epic id, counts by subsystem
and priority, the P0 list, out-of-scope ratification asks, and anything that smells
like a regression (UNWIRED/STUB where tests are green). The flywheel picks the beads
up through normal assignment — mention `/flywheel-start` if the swarm is not running.

## Hard Rules

- NEVER read Patina progress docs, prior audits, or beads as evidence of parity —
  fresh eyes every run. Scope docs (prd/, AGENTS.md, THIRDPARTY_STRATEGY.md) may be
  cited only for OUT_OF_SCOPE.
- NEVER run `cargo`, `rust_task.sh`, `nextest`, or any build/test anywhere in this
  skill — main loop and subagents alike. The verifier lane owns builds. The audit is
  static analysis; bead acceptance tests are run later by the flywheel.
- NEVER let a subagent run `br` or write files. All `br` writes happen serially in
  Phase 3, by you.
- NEVER wrap `br search` in a custom filter, and NEVER inspect candidate titles to
  second-guess a hit — `jq -r 'length'`, integer rule, done.
- NEVER create a bead with null/empty acceptance criteria, placeholder text, an
  unscoped test command, or meta work ("audit", "investigate", "align").
- NEVER silently drop coverage: dropped subsystems get re-run; capped or degraded
  anything is named in the report.
- NEVER mark OUT_OF_SCOPE without a citation, and never bead it — humans ratify scope.
- Fix root causes of tooling failures (bad grep, br flag drift → check `--help`);
  don't paper over them.

## Cost & cadence

Full run: ~24 finders + ~16 sweep triagers + ~25–60 verifiers + critic + supplements.
Run it after each large batch of work lands, before planning the next one. Between
full runs, prefer scoped runs (`/port-audit rendering-3d vfx-particles`) and
`/editor-parity` for pixel-level editor drift.
