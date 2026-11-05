#!/usr/bin/env node
// playtest-gate — play every real game in games/ through the web editor the
// way a person does, and fail when any of them does not work.
//
// Each game that has a `games/<game>/playtest.json` scenario gets a fresh
// release editor on its own port, then `browser-playtest` runs the scenario's
// keys in a real headless Chrome. The gate then judges the report:
//
//   functional  the scenario's `expect` rules (a watched value must increase /
//               decrease / change across a step), page exceptions and console
//               errors, editor-API 4xx/5xx, images that do not load, game
//               script errors
//   perf        frames shown below `min_fps`, `game-bound` / `render-bound`
//               findings, audio underruns above `max_audio_underruns`, render
//               time 25% over the recorded baseline, soak growth
//
// A perf failure on a busy host (1-minute load above 75% of the CPUs) is
// re-run once; still failing under load it is reported INCONCLUSIVE, not red,
// so other projects' builds do not file false regressions.
//
// Usage:
//   node scripts/playtest-gate.mjs [--games dodge_the_creeps,pong] [--soak 180]
//        [--build] [--file-beads] [--update-baselines] [--port-base 9500] [--json]
//
//   --soak <s>           also play each game for <s> seconds watching memory and
//                        render time (the nightly run uses 180)
//   --build              rebuild the release editor first (implied by both
//                        --update-* flags). Without it, a release editor older
//                        than the engine sources stops the gate (exit 2)
//   --allow-stale        measure a stale editor binary anyway
//   --file-beads         open a `br` bug (label playtest-gate) for each failing
//                        game, unless an open one with the same title exists
//   --update-baselines   record this run's render times as the new baselines
//                        (scripts/playtest-baselines.json)
//   --godot              also diff each game against REAL Godot (compare-to-godot
//                        with the scenario's keys and watches, on the same editor):
//                        per watched property the first divergence frame and max
//                        error, per shot frame the picture mismatch % and mean RGB
//                        delta. A score WORSE than scripts/godot-parity-baselines.json
//                        beyond the noise margin (scripts/lib/godot-parity.mjs) is a
//                        functional failure. Writes .playtest-gate/parity.md; with
//                        --file-beads files one bead per game for the WORST regression
//                        (label godot-parity). The Godot capture is cached under
//                        --godot-cache (default .playtest-gate/godot-cache) by a hash
//                        of (game files, scenario, Godot version, capture tool).
//   --godot-only         the --godot comparison without the browser playtest
//   --godot-cache <dir>  where Godot captures are cached
//   --update-godot-baselines  record this run's parity scores as the baselines.
//                        Only a person or the coordinator runs this, after merging
//                        (agents never do: it would hide their own regressions)
//
// A scenario is validated first (scene exists, steps parse, every `expect`
// names a step and a watched value): an invalid one is that game's ERROR.
// A scenario with `expect` rules runs browser-playtest with --expect-move, so a
// watched value that never changes is a functional failure.
//
// Writes .playtest-gate/latest.json (and a timestamped copy): the planner reads
// it. Exit 0 = all green, 1 = a failure, 2 = could not run, 3 = only
// inconclusive results.
//
// Scenario file (games/<game>/playtest.json):
//   { "scene": "main.tscn", "keys": "Enter:300,ArrowRight:1500",
//     "watch": ["/root/Main/Player:position"], "edit_steps": "select:TileMapLayer",
//     "audio": 3, "min_fps": 45, "max_audio_underruns": 0,
//     "expect": [{ "watch": "/root/Main/Player:position", "after": "ArrowRight:1500",
//                  "axis": 0, "change": "increase" }],
//     "allow_findings": [] }

import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import crypto from 'node:crypto';
import { resolveScenario, runGodotReference, defaultGodot } from './godot-reference.mjs';
import {
  hashGameFiles, captureCacheKey, captureComplete, scenarioKey, parityScores,
  compareParity, baselineEntry, regressionBeadTitle,
} from './lib/godot-parity.mjs';
import { PARITY_BASELINES, readBaselines, writeParityReport } from './parity-report.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const BASELINES = path.join(ROOT, 'scripts', 'playtest-baselines.json');
const OUT_DIR = path.join(ROOT, '.playtest-gate');

export function parseArgs(argv) {
  const o = { games: null, soak: 0, build: false, fileBeads: false, updateBaselines: false, portBase: 9500, json: false, godot: false, godotOnly: false, godotCache: path.join(OUT_DIR, 'godot-cache'), updateGodotBaselines: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const v = () => argv[++i];
    if (a === '--games') o.games = v().split(',').map((s) => s.trim()).filter(Boolean);
    else if (a === '--soak') o.soak = Number(v());
    else if (a === '--build') o.build = true;
    else if (a === '--file-beads') o.fileBeads = true;
    else if (a === '--update-baselines') o.updateBaselines = o.build = true;
    else if (a === '--port-base') o.portBase = Number(v());
    else if (a === '--json') o.json = true;
    else if (a === '--godot') o.godot = true;
    else if (a === '--godot-only') o.godot = o.godotOnly = true;
    else if (a === '--godot-cache') o.godotCache = path.resolve(v());
    // Recording baselines from a stale editor binary locks old behaviour in as
    // the reference (it happened: a pre-merge binary re-recorded every game),
    // so both update flags always rebuild first.
    else if (a === '--update-godot-baselines') o.updateGodotBaselines = o.godot = o.build = true;
    else if (a === '--allow-stale') o.allowStale = true;
    else if (a === '-h' || a === '--help') {
      console.log(fs.readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').slice(1).filter((l) => l.startsWith('//')).map((l) => l.replace(/^\/\/ ?/, '')).join('\n'));
      process.exit(0);
    } else {
      console.error(`playtest-gate: unknown option ${a}`);
      process.exit(2);
    }
  }
  return o;
}

export function loadScenarios(only, dir = path.join(ROOT, 'games')) {
  return fs.readdirSync(dir)
    .filter((g) => fs.existsSync(path.join(dir, g, 'playtest.json')))
    .filter((g) => !only || only.includes(g))
    .sort()
    .map((g) => {
      const base = { game: g, dir: path.join(dir, g) };
      // A broken scenario file is that game's ERROR, not a crash of the gate.
      let sc;
      try { sc = { ...base, ...JSON.parse(fs.readFileSync(path.join(dir, g, 'playtest.json'), 'utf8')) }; } catch (e) { return { ...base, invalid: [`playtest.json does not parse: ${e.message}`] }; }
      const invalid = validateScenario(sc);
      return invalid.length ? { ...sc, invalid } : sc;
    });
}

const STEP = /^(wait:\d+|(down|up):[A-Za-z0-9]+|[A-Za-z0-9]+:\d+)$/;

/// What is wrong with a scenario before anything runs: the scene must exist,
/// every key step must parse, and every `expect` rule must name a step the
/// keys run and a value the scenario watches.
export function validateScenario(sc) {
  const bad = [];
  if (!sc.scene) bad.push('no "scene"');
  else if (sc.dir && !fs.existsSync(path.join(sc.dir, sc.scene))) bad.push(`scene ${sc.scene} does not exist`);
  if (typeof sc.keys !== 'string' || !sc.keys.trim()) bad.push('no "keys"');
  const steps = String(sc.keys || '').split(',').map((x) => x.trim()).filter(Boolean);
  for (const st of steps) if (!STEP.test(st)) bad.push(`key step "${st}" is not Code:ms, down:Code, up:Code or wait:ms`);
  if (sc.watch != null && !Array.isArray(sc.watch)) bad.push('"watch" must be a list');
  for (const r of sc.expect || []) {
    if (!steps.includes(r.after)) bad.push(`expect after "${r.after}": no such step in keys`);
    if (!(sc.watch || []).includes(r.watch)) bad.push(`expect watch "${r.watch}" is not in "watch"`);
    if (!['increase', 'decrease', 'change'].includes(r.change)) bad.push(`expect change "${r.change}" is not increase/decrease/change`);
  }
  return bad;
}

/// The watched value after step number `i`, as numbers ([x, y] or a
/// scalar), or null. By INDEX, not label: a scenario may repeat a step
/// (`wait:800` twice), and a label lookup found the first one.
function watchedAt(report, watch, i) {
  const w = (report.watch || []).find((x) => x.watch === watch);
  const v = w?.values[i]?.value;
  if (v == null || String(v).startsWith('(')) return null;
  try { return JSON.parse(v); } catch { return null; }
}

/// Judge one `expect` rule: the value after `after` versus the step before it
/// (the last step with that label, so `after` may name a repeated step).
export function checkExpect(report, rule) {
  const steps = (report.steps || []).map((s) => s.step);
  const i = steps.lastIndexOf(rule.after);
  if (i < 0) return `expect: no step "${rule.after}" ran`;
  const now = watchedAt(report, rule.watch, i);
  const before = i > 0 ? watchedAt(report, rule.watch, i - 1) : null;
  if (now == null || before == null) {
    // Name what WAS read: "(no position: not on /root/Game/Player, a Node)"
    // says the player never instanced, which "not readable" alone hid.
    const raw = (report.watch || []).find((x) => x.watch === rule.watch)?.values[i]?.value;
    return `expect: ${rule.watch} was not readable around ${rule.after}${raw ? `: ${raw}` : ''}`;
  }
  const pick = (x) => (Array.isArray(x) ? x[rule.axis ?? 0] : x);
  const a = pick(before), b = pick(now);
  const ok = rule.change === 'increase' ? b > a : rule.change === 'decrease' ? b < a : JSON.stringify(now) !== JSON.stringify(before);
  return ok ? null : `expect: ${rule.watch}${Array.isArray(now) ? `[${rule.axis ?? 0}]` : ''} should ${rule.change} across ${rule.after} but went ${JSON.stringify(a)} → ${JSON.stringify(b)}`;
}

const PERF = /frames\/s, below --min-fps|^editor: (game|render)-bound|audio: the device ran dry|memory grew|render time grew|over baseline/;

/// Everything wrong with one run, split into functional and perf.
export function judge(sc, report, baseline) {
  const functional = [], perf = [];
  for (const p of report.problems || []) (PERF.test(p) ? perf : functional).push(p);
  for (const w of report.warnings || []) {
    if (/^editor: (game|render)-bound/.test(w) && !(sc.allow_findings || []).some((f) => w.includes(f))) perf.push(w);
  }
  for (const rule of sc.expect || []) {
    const bad = checkExpect(report, rule);
    if (bad) functional.push(bad);
  }
  const underruns = /underruns (\d+)/.exec(report.audio || '')?.[1];
  if (underruns != null && Number(underruns) > (sc.max_audio_underruns ?? 0)) perf.push(`audio: ${underruns} underruns (max ${sc.max_audio_underruns ?? 0})`);
  const ms = report.server?.render_ms;
  if (baseline?.render_ms && ms && ms > baseline.render_ms * 1.25 + 1) {
    perf.push(`render ${ms} ms, over baseline ${baseline.render_ms} ms (+25%)`);
  }
  return { functional: [...new Set(functional)], perf: [...new Set(perf)] };
}

function headCommit() {
  try { return execFileSync('git', ['rev-parse', '--short', 'HEAD'], { cwd: ROOT, encoding: 'utf8' }).trim(); } catch { return null; }
}

function sh(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', ...opts });
}

const TOOL_FILES = ['scripts/lib/godot-reference-capture.gd', 'scripts/godot-reference.mjs'];

/**
 * The Godot side of a game's parity check: the cached capture when one with
 * the same key exists, else a fresh capture into the cache. Returns
 * {dir, key, hit, scenario, version}.
 */
export function prepareReference(gameDir, cacheRoot, { godot = defaultGodot() } = {}) {
  const scenario = resolveScenario(gameDir, {});
  const version = spawnSync(godot, ['--version'], { encoding: 'utf8' }).stdout?.trim() || 'unknown';
  const tool = crypto.createHash('sha256').update(TOOL_FILES.map((f) => fs.readFileSync(path.join(ROOT, f))).join('\0')).digest('hex');
  const key = captureCacheKey({ files: hashGameFiles(gameDir), scenario, godotVersion: version, tool });
  const dir = path.join(cacheRoot, path.basename(gameDir), key);
  if (captureComplete(dir)) return { dir, key, hit: true, scenario, version };
  // Capture next to the final dir, then rename: a killed run never leaves a
  // half-written capture that a later run would take for a hit.
  const tmp = `${dir}.tmp-${process.pid}`;
  fs.rmSync(tmp, { recursive: true, force: true });
  runGodotReference(gameDir, { out: tmp, godot });
  fs.rmSync(dir, { recursive: true, force: true });
  fs.renameSync(tmp, dir);
  return { dir, key, hit: false, scenario, version };
}

/** compare-to-godot on the running editor against the (cached) reference; the scores and their comparison. */
function runParity(sc, port, o, base) {
  const gameDir = path.join(ROOT, 'games', sc.game);
  const out = path.join(OUT_DIR, 'parity', sc.game);
  let ref;
  try {
    ref = prepareReference(gameDir, o.godotCache);
  } catch (e) {
    return { error: `Godot capture failed: ${e.message.split('\n').slice(0, 6).join(' | ')}` };
  }
  const cache = { key: ref.key, hit: ref.hit, dir: ref.dir };
  const r = sh('node', ['scripts/compare-to-godot.mjs', `games/${sc.game}`, '--port', String(port), '--reference', ref.dir, '--out', out, '--json'], { timeout: 15 * 60 * 1000, maxBuffer: 64 << 20 });
  let report;
  try {
    if (r.status === 2) throw new Error('setup');
    report = JSON.parse(fs.readFileSync(path.join(out, 'report.json'), 'utf8'));
  } catch {
    return { cache, out, error: `compare-to-godot exited ${r.status}: ${(r.stderr || r.stdout || '').trim().slice(-400)}` };
  }
  const scores = parityScores(report, { scenario_key: scenarioKey(ref.scenario), godot_version: ref.version });
  return { cache, out, scores, comparison: compareParity(base, scores) };
}

function runOne(sc, port, soak, { playtest = true, parity = null } = {}) {
  const start = sh('scripts/editor-bg.sh', ['start', '--release', '--port', String(port), `games/${sc.game}`, sc.scene], { timeout: 120000 });
  if (start.status !== 0) return { error: `editor did not start: ${(start.stderr || start.stdout).trim().slice(-300)}` };
  if (!playtest) {
    try {
      return { parity: parity() };
    } finally {
      sh('scripts/editor-bg.sh', ['stop', '--port', String(port)]);
    }
  }
  const args = ['scripts/browser-playtest.mjs', '--url', `http://localhost:${port}/editor`, '--keys', sc.keys, '--json', '--min-fps', String(sc.min_fps ?? 45)];
  for (const w of sc.watch || []) args.push('--watch', w);
  if (sc.edit_steps) args.push('--edit-steps', sc.edit_steps);
  if (sc.audio) args.push('--audio', String(sc.audio));
  if (soak) args.push('--soak', String(soak));
  // The scenario expects the game to react: a watched value that never moves
  // is a functional failure, not a warning.
  if ((sc.expect || []).length) args.push('--expect-move');
  const r = sh('node', args, { timeout: 15 * 60 * 1000, maxBuffer: 64 << 20 });
  let report;
  try {
    report = JSON.parse(r.stdout);
  } catch {
    report = null;
  }
  try {
    if (!report) throw new Error('no report');
    // After the playtest, on the same editor: MCP `play` opens the main scene itself.
    return { report, parity: parity ? parity() : undefined };
  } catch {
    return { error: `browser-playtest exited ${r.status}: ${(r.stderr || r.stdout || '').trim().slice(-400)}` };
  } finally {
    sh('scripts/editor-bg.sh', ['stop', '--port', String(port)]);
  }
}

/** Files (or finds) the bead for a game's WORST parity regression. The title names the metric, not the numbers, so it dedupes run to run. */
function fileParityBead(game, result) {
  const c = result.parity.comparison;
  const worst = c.regressions[0];
  const title = regressionBeadTitle(game, worst);
  let open = [];
  try {
    open = JSON.parse(execFileSync('br', ['list', '--label', 'godot-parity', '--status', 'open', '--json'], { cwd: ROOT, encoding: 'utf8' }));
  } catch { /* no tracker: file anyway */ }
  const list = Array.isArray(open) ? open : open.issues || [];
  if (list.some((i) => i.title === title)) return `exists: ${title}`;
  const side = worst.metric.startsWith('picture') ? result.parity.scores.images[worst.id]?.side : null;
  const body = [
    `The Godot-parity gate (node scripts/playtest-gate.mjs --godot) found games/${game} got WORSE against real Godot than its recorded baseline (scripts/godot-parity-baselines.json).`,
    '', `Worst: ${worst.text}`, '', 'All regressions:', ...c.regressions.map((x) => `- ${x.text}`),
    ...(side ? ['', `Godot | Patina | heat: ${side}`] : []),
    '', `Report: .playtest-gate/parity.md, ${result.parity.out}/report.json`,
    `Reproduce: node scripts/playtest-gate.mjs --godot-only --games ${game}`,
    'Find the change that made it worse (the regression is relative to the baseline, not absolute). Do not re-record the baseline to close this.',
  ].join('\n');
  const id = execFileSync('br', ['create', '-t', 'bug', '-p', '1', '-l', `godot-parity,${game}`, '-d', body, '--silent', title], { cwd: ROOT, encoding: 'utf8' }).trim();
  return `filed ${id}: ${title}`;
}

const PARITY_TAG = 'godot-parity: ';

function fileBead(game, result) {
  const first = result.functional.find((x) => !x.startsWith(PARITY_TAG)) || result.perf[0] || result.error;
  const title = `[playtest-gate] ${game}: ${first}`.slice(0, 140);
  let open = [];
  try {
    open = JSON.parse(execFileSync('br', ['list', '--label', 'playtest-gate', '--status', 'open', '--json'], { cwd: ROOT, encoding: 'utf8' }));
  } catch { /* no tracker: file anyway */ }
  const list = Array.isArray(open) ? open : open.issues || [];
  if (list.some((i) => i.title === title)) return `exists: ${title}`;
  const body = [
    `The playtest gate (scripts/playtest-gate.mjs) failed for games/${game}.`,
    '', 'Functional:', ...result.functional.map((x) => `- ${x}`),
    '', 'Perf:', ...result.perf.map((x) => `- ${x}`),
    '', `Report: ${result.report_dir || 'n/a'}`,
    `Reproduce: node scripts/playtest-gate.mjs --games ${game}`,
  ].join('\n');
  const id = execFileSync('br', ['create', '-t', 'bug', '-p', result.functional.length ? '1' : '2', '-l', `playtest-gate,${game}`, '-d', body, '--silent', title], { cwd: ROOT, encoding: 'utf8' }).trim();
  return `filed ${id}: ${title}`;
}

/// Newest modification time (ms) of any engine source the editor binary is
/// built from: `engine-rs/crates/*/src/**`, `engine-rs/examples/**`, and the
/// crates' `Cargo.toml`s. Tests, fixtures and docs don't count.
export function newestEngineSourceMtime(engineDir = path.join(ROOT, 'engine-rs')) {
  let newest = 0;
  const walk = (dir) => {
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) walk(p);
      else if (/\.(rs|toml|wgsl|js|html|css)$/.test(e.name)) newest = Math.max(newest, fs.statSync(p).mtimeMs);
    }
  };
  const crates = path.join(engineDir, 'crates');
  for (const c of fs.existsSync(crates) ? fs.readdirSync(crates) : []) {
    walk(path.join(crates, c, 'src'));
    const toml = path.join(crates, c, 'Cargo.toml');
    if (fs.existsSync(toml)) newest = Math.max(newest, fs.statSync(toml).mtimeMs);
  }
  walk(path.join(engineDir, 'examples'));
  return newest;
}

/// The command that rebuilds the release editor, as [cmd, args, cwd]. `cmd` is
/// ABSOLUTE: spawned relative to `cwd: engine-rs` it resolved to a script that
/// doesn't exist, so `--build` failed with ENOENT and exited 2 silently.
export function editorBuildCommand() {
  return [path.join(ROOT, 'scripts', 'rust_task.sh'), ['build', '--release', '--example', 'editor'], path.join(ROOT, 'engine-rs')];
}

/// Null when `binary` is at least as new as the engine sources, else a message
/// saying how far behind it is. A missing binary counts as stale.
export function staleBinaryMessage(binary, newestSourceMs) {
  if (!fs.existsSync(binary)) return `${binary} does not exist`;
  const built = fs.statSync(binary).mtimeMs;
  if (built >= newestSourceMs) return null;
  const mins = Math.round((newestSourceMs - built) / 60000);
  return `${path.relative(ROOT, binary)} is ${mins} min older than the newest engine source; it would measure old code`;
}

async function main() {
  const o = parseArgs(process.argv.slice(2));
  const scenarios = loadScenarios(o.games);
  if (!scenarios.length) {
    console.error('playtest-gate: no games/<game>/playtest.json scenarios found');
    process.exit(2);
  }
  if (o.build) {
    const [cmd, args, cwd] = editorBuildCommand();
    const b = sh(cmd, args, { cwd, stdio: 'inherit' });
    if (b.status !== 0) {
      console.error(`playtest-gate: building the release editor failed (${b.error ? b.error.message : `exit ${b.status}`})`);
      process.exit(2);
    }
  }
  const stale = staleBinaryMessage(path.join(ROOT, 'engine-rs/target/release/examples/editor'), newestEngineSourceMtime());
  if (stale && !o.allowStale) {
    console.error(`playtest-gate: ${stale}. Pass --build to rebuild it (or --allow-stale to measure it anyway).`);
    process.exit(2);
  }
  const baselines = fs.existsSync(BASELINES) ? JSON.parse(fs.readFileSync(BASELINES, 'utf8')) : {};
  const parityBase = o.godot ? readBaselines() : {};
  const busy = () => os.loadavg()[0] > os.cpus().length * 0.75;
  const results = [];
  for (const [i, sc] of scenarios.entries()) {
    const port = o.portBase + i;
    if (sc.invalid) {
      const res = { game: sc.game, port, status: 'error', error: `invalid scenario: ${sc.invalid.join('; ')}`, functional: [], perf: [] };
      if (o.fileBeads) { try { res.bead = fileBead(sc.game, res); } catch (e) { res.bead = `could not file: ${e.message}`; } }
      results.push(res);
      if (!o.json) console.log(`${'ERROR'.padEnd(12)} ${sc.game}\n             - ${res.error}`);
      continue;
    }
    const parity = o.godot ? () => runParity(sc, port, o, parityBase[sc.game]) : null;
    let attempt = runOne(sc, port, o.soak, { playtest: !o.godotOnly, parity });
    let verdict = attempt.report ? judge(sc, attempt.report, baselines[sc.game]) : o.godotOnly && attempt.parity ? { functional: [], perf: [] } : null;
    let rerun = false;
    if (attempt.report && verdict && !verdict.functional.length && verdict.perf.length && busy()) {
      rerun = true;
      const again = runOne(sc, port, o.soak);
      if (again.report) {
        attempt = { ...again, parity: attempt.parity };
        verdict = judge(sc, again.report, baselines[sc.game]);
      }
    }
    const res = { game: sc.game, port, rerun, load1: +os.loadavg()[0].toFixed(1), cpus: os.cpus().length };
    if (attempt.parity) {
      res.parity = attempt.parity;
      const c = attempt.parity.comparison;
      if (verdict) {
        if (attempt.parity.error) verdict.functional.push(`${PARITY_TAG}${attempt.parity.error}`);
        for (const x of c?.regressions || []) verdict.functional.push(`${PARITY_TAG}${x.text}`);
      }
      if (o.updateGodotBaselines && attempt.parity.scores) {
        parityBase[sc.game] = baselineEntry(attempt.parity.scores, { recorded: new Date().toISOString().slice(0, 10), commit: headCommit(), margins: parityBase[sc.game]?.margins || null });
      }
    }
    if (!verdict) Object.assign(res, { status: 'error', error: attempt.error || attempt.parity?.error, functional: [], perf: [] });
    else {
      const r = attempt.report;
      Object.assign(res, verdict, {
        status: verdict.functional.length ? 'fail' : verdict.perf.length ? (busy() ? 'inconclusive' : 'fail') : 'pass',
      });
      if (r) Object.assign(res, {
        shown_fps: r.shown_fps, render_ms: r.server?.render_ms, sim_fps: r.server?.sim_fps,
        soak_rss_mb_per_min: r.soak_rss_mb_per_min ?? null, report_dir: r.screenshots?.[0] ? path.dirname(r.screenshots[0]) : null,
      });
      if (o.updateBaselines && r && res.status !== 'error' && r.server?.render_ms) baselines[sc.game] = { render_ms: r.server.render_ms, recorded: new Date().toISOString().slice(0, 10) };
    }
    // The playtest bead is for playtest problems; parity regressions get their own bead (worst one per game).
    const playtestBad = res.status === 'error' || res.perf.length || res.functional.some((x) => !x.startsWith(PARITY_TAG));
    if (o.fileBeads && (res.status === 'fail' || res.status === 'error') && playtestBad) {
      try { res.bead = fileBead(sc.game, res); } catch (e) { res.bead = `could not file: ${e.message}`; }
    }
    if (o.fileBeads && res.parity?.comparison?.regressions.length) {
      try { res.parity_bead = fileParityBead(sc.game, res); } catch (e) { res.parity_bead = `could not file: ${e.message}`; }
    }
    results.push(res);
    if (!o.json) {
      const tag = { pass: 'PASS', fail: 'FAIL', inconclusive: 'INCONCLUSIVE', error: 'ERROR' }[res.status];
      console.log(`${tag.padEnd(12)} ${sc.game}${res.shown_fps != null ? `  ${res.shown_fps} fps shown, render ${res.render_ms} ms` : ''}${rerun ? '  (re-run: host busy)' : ''}`);
      for (const x of [...(res.functional || []), ...(res.perf || [])]) console.log(`             - ${x}`);
      if (res.error) console.log(`             - ${res.error}`);
      if (res.bead) console.log(`             bead: ${res.bead}`);
      const pc = res.parity?.comparison;
      if (pc) console.log(`             godot parity: ${pc.status}, ${pc.regressions.length} worse / ${pc.improvements.length} better (Godot capture ${res.parity.cache.hit ? 'cached' : 'fresh'})${pc.stale.length ? `; ${pc.stale.join('; ')}` : ''}`);
      if (res.parity_bead) console.log(`             parity bead: ${res.parity_bead}`);
    }
  }
  if (o.updateBaselines) fs.writeFileSync(BASELINES, JSON.stringify(baselines, null, 2) + '\n');
  if (o.updateGodotBaselines) {
    let doc = {};
    try { doc = JSON.parse(fs.readFileSync(PARITY_BASELINES, 'utf8')); } catch { /* first record */ }
    doc._doc = doc._doc || 'Godot-parity baselines for node scripts/playtest-gate.mjs --godot. Lower is better. Re-record ONLY as a person or the coordinator: node scripts/playtest-gate.mjs --godot-only --update-godot-baselines';
    doc.games = { ...(doc.games || {}), ...parityBase };
    fs.writeFileSync(PARITY_BASELINES, JSON.stringify(doc, null, 2) + '\n');
  }
  fs.mkdirSync(OUT_DIR, { recursive: true });
  const summary = { at: new Date().toISOString(), soak: o.soak, results };
  fs.writeFileSync(path.join(OUT_DIR, 'latest.json'), JSON.stringify(summary, null, 2));
  fs.writeFileSync(path.join(OUT_DIR, `${summary.at.replace(/[:.]/g, '-')}.json`), JSON.stringify(summary, null, 2));
  if (o.godot) {
    const { out } = writeParityReport({ latest: summary, baselines: readBaselines() });
    if (!o.json) console.log(`godot parity report: ${path.relative(ROOT, out)}`);
  }
  if (o.json) console.log(JSON.stringify(summary, null, 2));
  const st = results.map((r) => r.status);
  process.exit(st.some((s) => s === 'fail' || s === 'error') ? 1 : st.some((s) => s === 'inconclusive') ? 3 : 0);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
