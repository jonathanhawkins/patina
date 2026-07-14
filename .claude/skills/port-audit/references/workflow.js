export const meta = {
  name: 'port-audit',
  description: 'Fresh-eyes 1:1 Godot->Patina parity audit: per-subsystem finders, full class sweep, adversarial verify, completeness critic',
  phases: [
    { title: 'Inventory', detail: 'one fresh-eyes finder per subsystem' },
    { title: 'Verify', detail: 'adversarial refutation of every finding' },
    { title: 'Sweep', detail: 'triage every Godot class unaccounted for' },
    { title: 'Critic', detail: 'completeness check + supplemental finders' },
  ],
}

// args: { runDate, root, runDir, api, taxonomy, subsystems: [key,...],
//         sweepChunks: [[className,...],...], godotVersion, skipCritic? }
// The harness may deliver args as a JSON-encoded string — tolerate both forms.
const cfg = typeof args === 'string' ? JSON.parse(args) : args
if (!cfg || !Array.isArray(cfg.subsystems)) {
  throw new Error(`port-audit: bad args — expected {subsystems: [...]}, got ${typeof args}`)
}

const STATUS = ['MISSING', 'STUB', 'UNWIRED', 'PARTIAL', 'OUT_OF_SCOPE']

const BEAD_SCHEMA = {
  type: 'object',
  required: ['title', 'description', 'acceptance_criteria', 'test_name', 'type'],
  properties: {
    title: { type: 'string' },
    description: { type: 'string' },
    files: { type: 'array', items: { type: 'string' } },
    acceptance_criteria: { type: 'string' },
    test_name: { type: 'string' },
    type: { enum: ['bug', 'feature', 'task'] },
  },
}

const FINDING_SCHEMA = {
  type: 'object',
  required: ['feature', 'status', 'godot_ref', 'evidence', 'priority', 'group_key'],
  properties: {
    feature: { type: 'string' },
    godot_classes: { type: 'array', items: { type: 'string' } },
    godot_ref: { type: 'string' },
    status: { enum: STATUS },
    evidence: { type: 'string' },
    patina_paths: { type: 'array', items: { type: 'string' } },
    priority: { type: 'integer', minimum: 0, maximum: 3 },
    scope_citation: { type: 'string' },
    group_key: { type: 'string' },
    bead: BEAD_SCHEMA,
  },
}

const FINDER_SCHEMA = {
  type: 'object',
  required: ['subsystem', 'findings', 'parity_ok', 'coverage'],
  properties: {
    subsystem: { type: 'string' },
    findings: { type: 'array', items: FINDING_SCHEMA },
    parity_ok: { type: 'array', items: { type: 'string' } },
    coverage: { type: 'string' },
  },
}

const VERIFY_SCHEMA = {
  type: 'object',
  required: ['verdicts'],
  properties: {
    verdicts: {
      type: 'array',
      items: {
        type: 'object',
        required: ['feature', 'verdict', 'note'],
        properties: {
          feature: { type: 'string' },
          verdict: { enum: ['CONFIRMED', 'REFUTED', 'ADJUSTED'] },
          corrected_status: { enum: STATUS },
          corrected_priority: { type: 'integer', minimum: 0, maximum: 3 },
          note: { type: 'string' },
        },
      },
    },
  },
}

const CRITIC_SCHEMA = {
  type: 'object',
  required: ['missed_areas'],
  properties: {
    missed_areas: {
      type: 'array',
      items: {
        type: 'object',
        required: ['key', 'why', 'ground_truth'],
        properties: {
          key: { type: 'string' },
          why: { type: 'string' },
          ground_truth: { type: 'string' },
        },
      },
    },
    notes: { type: 'string' },
  },
}

const chunk = (arr, n) => {
  const out = []
  for (let i = 0; i < arr.length; i += n) out.push(arr.slice(i, i + n))
  return out
}

// Retry once on null (skipped/terminal-error agents).
async function runAgent(prompt, opts) {
  const r = await agent(prompt, opts)
  if (r) return r
  log(`retrying ${opts.label}`)
  return agent(prompt, { ...opts, label: `${opts.label}:retry` })
}

const COMMON_RULES = `
HARD RULES (all stages):
- READ-ONLY: never run cargo, rust_task.sh, nextest, pnpm, npm, git-mutating commands,
  or br. Never write/edit any file. Allowed: Read, Glob, Grep, ls, jq, and read-only
  shell inspection.
- FRESH EYES: Patina's own claims are under test, not evidence. Do NOT read
  COMPAT_MATRIX.md, COMPAT_DASHBOARD.md, docs/audits/, prd/ progress maps, .beads/,
  planner logs, or any parity report. Exception: prd/ scope docs, AGENTS.md, and
  THIRDPARTY_STRATEGY.md may be cited ONLY as scope declarations for OUT_OF_SCOPE
  findings — never as evidence that something works.
- Ground truth is Godot 4.6.1 itself: the extension API dump, upstream/godot C++
  source, and upstream/godot/doc/classes/*.xml.
- Tests passing against a module is NOT evidence a feature is wired into the runtime.
- Evidence or it did not happen: cite file:line for what exists; for absence claims,
  list the exact grep patterns you ran.
- Return your result ONLY via the StructuredOutput tool.`

const finderPrompt = (key, extra) => `
You are one finder in a fresh-eyes 1:1 port audit of the Patina engine (Rust,
${cfg.root}/engine-rs) against Godot ${cfg.godotVersion}. Your subsystem: "${key}".
${extra}
Step 1 — Read ${cfg.taxonomy} and locate your section "${key}" (read the top-of-file
"Scope anchor" and "Cross-cutting" sections too). Its bullets are your coverage FLOOR.
Step 2 — Build the Godot-side inventory from ground truth: slice ${cfg.api}
(extension_api.json: classes, methods, properties, signals, enums, utility_functions,
singletons) with jq, and read upstream C++/doc XML under ${cfg.root}/upstream/godot/
for behavioral detail.
Step 3 — Hunt each item in Patina: ${cfg.root}/engine-rs/crates/* (taxonomy lists
entry-point hints; verify them). Search hard before claiming absence: exact Godot
names, snake_case variants, abbreviations, re-exports, and the registration tables in
engine-rs/crates/gdobject/src/class_db.rs (register_* functions).
Step 4 — Classify every non-parity item (the ladder):
  MISSING  - no meaningful implementation exists.
  STUB     - exists but returns defaults / todo!/unimplemented!/empty bodies.
  UNWIRED  - real implementation exists but NO call path from the runtime reaches it
             (patina-runner frame loop, gdscene MainLoop/SceneTree tick, editor server
             routes, served EDITOR_HTML JS). This is the known systemic failure here -
             check call sites outside the defining module and outside #[cfg(test)].
  PARTIAL  - wired, but significant Godot behaviors/parameters are absent or wrong.
  OUT_OF_SCOPE - ONLY with a scope_citation (Godot web-limitations, prd/ doc,
             THIRDPARTY_STRATEGY.md). No citation => it is a gap, not out of scope.
Items at genuine parity go in parity_ok (short strings), not findings.
Step 5 — For each finding EXCEPT OUT_OF_SCOPE draft ONE bead (concrete IMPLEMENTATION
work, never "investigate/audit X"):
  - title: "<Area>: <imperative concrete change>" (repo style, e.g. "Physics2D:
    rotation-aware SAT narrowphase + missing shapes").
  - description: Godot ground-truth behavior (+ upstream ref), Patina current state
    with file:line evidence, concrete implementation approach naming real files, and
    the wiring requirement (the call path that must exist end-to-end).
  - acceptance_criteria: observable app-level behavior + a scoped test named like
    (test: \`snake_case_name\`) runnable via cargo nextest --test <name>. The test must
    prove WIRING (e.g. scene-load-to-output), not just module math.
  - test_name: that snake_case test binary name.
  - type: bug for UNWIRED/STUB-behind-green-tests, feature for MISSING/PARTIAL.
  - Cluster small related gaps (a family of classes/params) into one worker-session
    sized bead via a shared group_key; split monsters into staged beads.
Priority rubric: 0 = core runtime/editor path broken OR false "done" (unwired/stub
behind passing tests); 1 = commonly-used Godot feature absent/partial; 2 = less
common; 3 = exotic/polish. Be honest - not everything is P0.
Finish with a candid "coverage" statement: what you did NOT get to.
${COMMON_RULES}`

const verifyPrompt = (key, findings) => `
You are an adversarial verifier in a Godot->Patina port audit. A finder for subsystem
"${key}" produced the findings below. Your job is to REFUTE each one.
For each finding:
- MISSING: search harder than the finder. Alternate spellings, snake_case, partial
  names, re-exports, other crates, macro-generated registrations, class_db.rs tables.
- UNWIRED: hunt for ANY real runtime call path they missed: patina-runner loop,
  gdscene MainLoop/SceneTree tick, editor server route handlers, served EDITOR_HTML
  JS (grep the pub const EDITOR_HTML string), signal connections, registered process
  callbacks. #[cfg(test)]-only callers do not count as wiring.
- STUB: check whether a real implementation lives elsewhere (trait impls, other crate).
- PARTIAL: spot-check 2-3 of the claimed-missing behaviors against the code.
- OUT_OF_SCOPE: verify the citation actually says that; if not, refute the scoping.
Also sanity-check priority against the rubric (0 broken-core/false-done, 1 common,
2 uncommon, 3 exotic) and the status label itself.
Verdicts: CONFIRMED (your refutation failed - say what you tried), REFUTED (proof with
file:line), ADJUSTED (finding is real but status/priority wrong - supply corrections).
Echo each finding's "feature" string EXACTLY as given.
FINDINGS:
${JSON.stringify(findings, null, 1)}
${COMMON_RULES}`

const sweepPrompt = (names, i) => `
You are sweep-triage agent #${i} in a Godot->Patina port audit. Below are Godot
${cfg.godotVersion} class names with NO registration found in Patina's ClassDB
(engine-rs/crates/gdobject/src/class_db.rs). The mechanical diff may be wrong - your
first job is skepticism, your second is triage.
For EACH class:
1. Search Patina for it anyway (exact, snake_case, partial, other crates, other
   registration sites). If implemented+registered, drop it (mention in coverage).
2. Otherwise read its role in ${cfg.api} / ${cfg.root}/upstream/godot/doc/classes/
   and classify per the ladder (usually MISSING) with an honest priority (many of
   these are exotic: editor-internal, VisualShaderNode*, OpenXR* - use the taxonomy
   scope anchor at the top of ${cfg.taxonomy}; OUT_OF_SCOPE needs a citation).
3. GROUP related classes into ONE finding/bead per coherent family (e.g. "GraphEdit
   family", "AudioEffect EQ family") via group_key - do not emit 40 one-class beads.
   Set the finding's feature to the family name and list all classes in godot_classes.
Bead drafting rules, priority rubric, and output shape are the same as for finders.
Set the top-level "subsystem" field to "sweep-${i}". Each finding's group_key MUST be
namespaced with the best-matching taxonomy key: "<taxonomy-key>/<family-slug>"
(e.g. "audio/effect-eq-family") - the synthesis stage labels beads from that prefix.
CLASSES:
${JSON.stringify(names)}
${COMMON_RULES}`

const criticPrompt = (coverages, doneKeys) => `
You are the completeness critic for a fresh-eyes Godot->Patina port audit. The goal
was: leave NO stone unturned across the entire Godot ${cfg.godotVersion} surface
(runtime per Godot's web-export capability, editor per desktop Godot).
Subsystems audited: ${JSON.stringify(doneKeys)}.
Finder coverage confessions (what they admit skipping):
${JSON.stringify(coverages, null, 1)}
Cross-check against ground truth (${cfg.api}, ${cfg.root}/upstream/godot/): name
concrete areas the audit MISSED - a modality nobody ran (singletons? utility
functions? project settings? default theme? import formats? notification order?), a
subsystem absent from the list, or a confessed skip that matters. Each missed_area
becomes a supplemental finder: give it a short key, why it matters, and a
ground_truth pointer (file/jq path). Return at most 6, most important first. If the
audit is genuinely complete, return an empty list - do not invent work.
${COMMON_RULES}`

// One subsystem end-to-end: find, then verify in chunks (no cross-subsystem barrier).
async function auditSubsystem(key, extra) {
  const found = await runAgent(finderPrompt(key, extra || ''), {
    label: `find:${key}`, phase: 'Inventory', schema: FINDER_SCHEMA,
  })
  if (!found) return null
  const findings = found.findings || []
  log(`${key}: ${findings.length} findings, ${(found.parity_ok || []).length} at parity`)
  if (findings.length === 0) return { ...found, verdicts: [] }
  const verdicts = (await parallel(
    chunk(findings, 8).map((c, i) => () =>
      runAgent(verifyPrompt(key, c), {
        label: `verify:${key}.${i}`, phase: 'Verify', schema: VERIFY_SCHEMA,
      })),
  )).filter(Boolean).flatMap(v => v.verdicts || [])
  return { ...found, verdicts }
}

// ---- Inventory + Verify (per-subsystem chains run concurrently) ----
const subsystemResults = await parallel(
  cfg.subsystems.map(key => () => auditSubsystem(key, '')),
)

// ---- Sweep (self-verifying triage of unregistered classes) ----
const sweepChunks = cfg.sweepChunks || []
const sweepResults = (await parallel(
  sweepChunks.map((names, i) => () =>
    runAgent(sweepPrompt(names, i), {
      label: `sweep:${i}`, phase: 'Sweep', schema: FINDER_SCHEMA,
    })),
)).filter(Boolean)

const dropped = cfg.subsystems.filter((k, i) => !subsystemResults[i])
if (dropped.length) log(`DROPPED subsystems (retried once, still failed): ${dropped.join(', ')}`)

// ---- Critic + one supplemental round ----
const okResults = subsystemResults.filter(Boolean)
let critique = null
let supplemental = []
if (!cfg.skipCritic) {
  const coverages = okResults.map(r => ({ subsystem: r.subsystem, coverage: r.coverage }))
    .concat(sweepResults.map(r => ({ subsystem: `sweep:${r.subsystem}`, coverage: r.coverage })))
  critique = await runAgent(criticPrompt(coverages, cfg.subsystems), {
    label: 'critic', phase: 'Critic', schema: CRITIC_SCHEMA,
  })
  const missed = ((critique && critique.missed_areas) || []).slice(0, 6)
  if (missed.length) {
    log(`critic found ${missed.length} missed areas: ${missed.map(m => m.key).join(', ')}`)
    supplemental = (await parallel(
      missed.map(m => () => auditSubsystem(m.key,
        `This is a SUPPLEMENTAL pass - the completeness critic flagged this area as
missed. Why: ${m.why}. Ground truth pointer: ${m.ground_truth}. Your taxonomy section
may not exist; derive the beat map from the ground truth pointer + scope anchor.`)),
    )).filter(Boolean)
  }
}

return {
  runDate: cfg.runDate,
  godotVersion: cfg.godotVersion,
  subsystems: okResults,
  sweep: sweepResults,
  supplemental,
  critique,
  dropped,
}
