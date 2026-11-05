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
//   --build              rebuild the release editor first
//   --file-beads         open a `br` bug (label playtest-gate) for each failing
//                        game, unless an open one with the same title exists
//   --update-baselines   record this run's render times as the new baselines
//                        (scripts/playtest-baselines.json)
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

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const BASELINES = path.join(ROOT, 'scripts', 'playtest-baselines.json');
const OUT_DIR = path.join(ROOT, '.playtest-gate');

export function parseArgs(argv) {
  const o = { games: null, soak: 0, build: false, fileBeads: false, updateBaselines: false, portBase: 9500, json: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const v = () => argv[++i];
    if (a === '--games') o.games = v().split(',').map((s) => s.trim()).filter(Boolean);
    else if (a === '--soak') o.soak = Number(v());
    else if (a === '--build') o.build = true;
    else if (a === '--file-beads') o.fileBeads = true;
    else if (a === '--update-baselines') o.updateBaselines = true;
    else if (a === '--port-base') o.portBase = Number(v());
    else if (a === '--json') o.json = true;
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

function sh(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', ...opts });
}

function runOne(sc, port, soak) {
  const start = sh('scripts/editor-bg.sh', ['start', '--release', '--port', String(port), `games/${sc.game}`, sc.scene], { timeout: 120000 });
  if (start.status !== 0) return { error: `editor did not start: ${(start.stderr || start.stdout).trim().slice(-300)}` };
  const args = ['scripts/browser-playtest.mjs', '--url', `http://localhost:${port}/editor`, '--keys', sc.keys, '--json', '--min-fps', String(sc.min_fps ?? 45)];
  for (const w of sc.watch || []) args.push('--watch', w);
  if (sc.edit_steps) args.push('--edit-steps', sc.edit_steps);
  if (sc.audio) args.push('--audio', String(sc.audio));
  if (soak) args.push('--soak', String(soak));
  // The scenario expects the game to react: a watched value that never moves
  // is a functional failure, not a warning.
  if ((sc.expect || []).length) args.push('--expect-move');
  const r = sh('node', args, { timeout: 15 * 60 * 1000, maxBuffer: 64 << 20 });
  try {
    return { report: JSON.parse(r.stdout) };
  } catch {
    return { error: `browser-playtest exited ${r.status}: ${(r.stderr || r.stdout || '').trim().slice(-400)}` };
  } finally {
    sh('scripts/editor-bg.sh', ['stop', '--port', String(port)]);
  }
}

function fileBead(game, result) {
  const first = result.functional[0] || result.perf[0] || result.error;
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

async function main() {
  const o = parseArgs(process.argv.slice(2));
  const scenarios = loadScenarios(o.games);
  if (!scenarios.length) {
    console.error('playtest-gate: no games/<game>/playtest.json scenarios found');
    process.exit(2);
  }
  if (o.build) {
    const b = sh('scripts/rust_task.sh', ['build', '--release', '--example', 'editor'], { cwd: path.join(ROOT, 'engine-rs'), stdio: 'inherit' });
    if (b.status !== 0) process.exit(2);
  }
  const baselines = fs.existsSync(BASELINES) ? JSON.parse(fs.readFileSync(BASELINES, 'utf8')) : {};
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
    let attempt = runOne(sc, port, o.soak);
    let verdict = attempt.report ? judge(sc, attempt.report, baselines[sc.game]) : null;
    let rerun = false;
    if (verdict && !verdict.functional.length && verdict.perf.length && busy()) {
      rerun = true;
      attempt = runOne(sc, port, o.soak);
      verdict = attempt.report ? judge(sc, attempt.report, baselines[sc.game]) : null;
    }
    const res = { game: sc.game, port, rerun, load1: +os.loadavg()[0].toFixed(1), cpus: os.cpus().length };
    if (!verdict) Object.assign(res, { status: 'error', error: attempt.error, functional: [], perf: [] });
    else {
      const r = attempt.report;
      Object.assign(res, verdict, {
        status: verdict.functional.length ? 'fail' : verdict.perf.length ? (busy() ? 'inconclusive' : 'fail') : 'pass',
        shown_fps: r.shown_fps, render_ms: r.server?.render_ms, sim_fps: r.server?.sim_fps,
        soak_rss_mb_per_min: r.soak_rss_mb_per_min ?? null, report_dir: r.screenshots?.[0] ? path.dirname(r.screenshots[0]) : null,
      });
      if (o.updateBaselines && res.status !== 'error' && r.server?.render_ms) baselines[sc.game] = { render_ms: r.server.render_ms, recorded: new Date().toISOString().slice(0, 10) };
    }
    if (o.fileBeads && (res.status === 'fail' || res.status === 'error')) {
      try { res.bead = fileBead(sc.game, res); } catch (e) { res.bead = `could not file: ${e.message}`; }
    }
    results.push(res);
    if (!o.json) {
      const tag = { pass: 'PASS', fail: 'FAIL', inconclusive: 'INCONCLUSIVE', error: 'ERROR' }[res.status];
      console.log(`${tag.padEnd(12)} ${sc.game}${res.shown_fps != null ? `  ${res.shown_fps} fps shown, render ${res.render_ms} ms` : ''}${rerun ? '  (re-run: host busy)' : ''}`);
      for (const x of [...(res.functional || []), ...(res.perf || [])]) console.log(`             - ${x}`);
      if (res.error) console.log(`             - ${res.error}`);
      if (res.bead) console.log(`             bead: ${res.bead}`);
    }
  }
  if (o.updateBaselines) fs.writeFileSync(BASELINES, JSON.stringify(baselines, null, 2) + '\n');
  fs.mkdirSync(OUT_DIR, { recursive: true });
  const summary = { at: new Date().toISOString(), soak: o.soak, results };
  fs.writeFileSync(path.join(OUT_DIR, 'latest.json'), JSON.stringify(summary, null, 2));
  fs.writeFileSync(path.join(OUT_DIR, `${summary.at.replace(/[:.]/g, '-')}.json`), JSON.stringify(summary, null, 2));
  if (o.json) console.log(JSON.stringify(summary, null, 2));
  const st = results.map((r) => r.status);
  process.exit(st.some((s) => s === 'fail' || s === 'error') ? 1 : st.some((s) => s === 'inconclusive') ? 3 : 0);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
