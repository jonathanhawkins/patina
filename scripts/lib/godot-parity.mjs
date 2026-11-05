// godot-parity: the pure logic of the Godot-parity regression gate
// (`node scripts/playtest-gate.mjs --godot`).
//
// A compare-to-godot report is reduced to a few SCORES per game (per watched
// property: first divergence frame and max error; per picture: mismatch % and
// mean RGB delta). Scores are compared with the recorded baselines in
// scripts/godot-parity-baselines.json. A score that got WORSE by more than a
// noise margin is a regression (a functional failure of the gate); one that
// got better beyond the margin is an improvement, reported so the baseline
// can be re-recorded. Nothing here touches the network, Godot or the editor:
// scripts/tests/godot-parity.test.mjs pins it.

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { decodePng } from './godot-reference-lib.mjs';

/**
 * How far a score may move before it counts. Every metric gets
 * `abs + rel * baseline` of slack:
 * - trace max error is in the property's units (pixels in 2D, metres in 3D);
 * - divergence is in frames (an EARLIER first divergence is worse);
 * - mismatch is in percentage points of pixels;
 * - mean delta is the mean RGB distance (0..441).
 * Godot is deterministic (byte-identical PNGs run to run) and so is MCP play,
 * so the margins only absorb small numeric drift, not run-to-run noise.
 * A game may override any of them under `margins` in the baselines file.
 */
export const DEFAULT_MARGINS = Object.freeze({
  traceMaxErrorAbs: 0.05,
  traceMaxErrorRel: 0.1,
  divergenceFrames: 2,
  mismatchPctAbs: 2,
  mismatchPctRel: 0,
  meanDeltaAbs: 2,
  meanDeltaRel: 0.05,
});

const sha = (data) => crypto.createHash('sha256').update(data).digest('hex');

/**
 * Every file of a game as [relative path, sha256], sorted. Hidden entries are
 * skipped: `.godot/` is Godot's import cache and `.editor/` is the Patina
 * editor's own state, which it writes into the game dir on every start (a key
 * that included it would miss the cache every run). Godot's import ignores
 * hidden files too.
 */
export function hashGameFiles(dir) {
  const out = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.name.startsWith('.')) continue;
      if (e.isDirectory()) walk(p);
      else if (e.isFile()) out.push([path.relative(dir, p).split(path.sep).join('/'), sha(fs.readFileSync(p))]);
    }
  };
  walk(dir);
  return out.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
}

/** The parts of a resolved scenario that change what Godot records. */
export function scenarioIdentity(sc) {
  return {
    scene: sc.scene,
    keys: sc.keys,
    fps: sc.fps,
    frames: sc.frames,
    size: sc.size,
    shots: sc.shots,
    watches: (sc.watches || []).map((w) => w.id ?? w),
    // Only a NON-default seed is part of the identity, so every baseline
    // recorded before seeds existed (all at the default) keeps its key.
    ...(sc.seed !== undefined && sc.seed !== 42 ? { seed: sc.seed } : {}),
  };
}

/** A short hash of the scenario alone: a baseline recorded under another scenario is not comparable. */
export function scenarioKey(sc) {
  return sha(JSON.stringify(scenarioIdentity(sc))).slice(0, 16);
}

/**
 * The cache key of a Godot capture: (game files, scenario, Godot version,
 * capture tool). Same key = Godot would record the same thing, so the
 * reference capture is reused.
 */
export function captureCacheKey({ files, scenario, godotVersion, tool = '' }) {
  const body = JSON.stringify({ files, scenario: scenarioIdentity(scenario), godot: String(godotVersion).trim(), tool });
  return sha(body).slice(0, 16);
}

/** A cached capture dir is usable when it has a trace and every shot it recorded. */
export function captureComplete(dir) {
  try {
    const trace = JSON.parse(fs.readFileSync(path.join(dir, 'trace.json'), 'utf8'));
    const shots = trace.meta?.shots || [];
    return shots.every((f) => fs.existsSync(path.join(dir, 'frames', `f${String(f).padStart(5, '0')}.png`)));
  } catch {
    return false;
  }
}

/**
 * Whether two Godot captures of one scenario recorded the same thing
 * (pat-y2ylj): every watched value of every frame, and every picture byte for
 * byte. A reference that differs between two runs cannot judge Patina — the
 * gate once flagged dodge_the_creeps f258 10.414 -> 15.050 when Patina's frame
 * was pixel-identical and only Godot's own unseeded RNG had moved. Returns
 * {stable, differences: [string]} (at most `limit` differences).
 */
export function compareCaptures(dirA, dirB, { limit = 8, pixelNoise = 2 } = {}) {
  const differences = [];
  const read = (d) => JSON.parse(fs.readFileSync(path.join(d, 'trace.json'), 'utf8'));
  let a;
  let b;
  try {
    a = read(dirA);
    b = read(dirB);
  } catch (e) {
    return { stable: false, differences: [`unreadable trace: ${e.message}`] };
  }
  const fa = a.frames || [];
  const fb = b.frames || [];
  if (fa.length !== fb.length) differences.push(`recorded ${fa.length} vs ${fb.length} frames`);
  for (let i = 0; i < Math.min(fa.length, fb.length) && differences.length < limit; i++) {
    const va = fa[i].values || {};
    const vb = fb[i].values || {};
    for (const id of new Set([...Object.keys(va), ...Object.keys(vb)])) {
      if (JSON.stringify(va[id]) !== JSON.stringify(vb[id])) {
        differences.push(`f${fa[i].frame} ${id}: ${JSON.stringify(va[id])} vs ${JSON.stringify(vb[id])}`);
        break;
      }
    }
  }
  const pngs = (d) => {
    try {
      return fs.readdirSync(path.join(d, 'frames')).filter((f) => f.endsWith('.png')).sort();
    } catch {
      return [];
    }
  };
  const pa = pngs(dirA);
  const pb = pngs(dirB);
  if (pa.join() !== pb.join()) differences.push(`pictures ${pa.join(',') || 'none'} vs ${pb.join(',') || 'none'}`);
  for (const f of pa.filter((x) => pb.includes(x))) {
    if (differences.length >= limit) break;
    const x = fs.readFileSync(path.join(dirA, 'frames', f));
    const y = fs.readFileSync(path.join(dirB, 'frames', f));
    if (x.equals(y)) continue;
    // Same pixels in a different encoding still count as the same picture,
    // and so does GPU rounding noise up to `pixelNoise` per channel.
    let worst = Infinity;
    let off = 0;
    try {
      const ia = decodePng(x);
      const ib = decodePng(y);
      if (ia.width === ib.width && ia.height === ib.height) {
        worst = 0;
        for (let i = 0; i < ia.data.length; i++) {
          const d = Math.abs(ia.data[i] - ib.data[i]);
          if (d > pixelNoise) off++;
          if (d > worst) worst = d;
        }
      }
    } catch {
      // undecodable: counts as different
    }
    if (worst > pixelNoise) differences.push(`picture ${f} differs (${Number.isFinite(worst) ? `${off} channel values beyond ${pixelNoise}, up to ${worst} apart` : 'another size or undecodable'})`);
  }
  return { stable: differences.length === 0, differences };
}

const finiteOrNull = (x) => (typeof x === 'number' && Number.isFinite(x) ? x : null);
const round = (x, d = 4) => (x === null ? null : Number(x.toFixed(d)));

/**
 * The scores of one compare-to-godot report.json. `max_error: null` means the
 * error was infinite (a side could not read the property). Picture scores are
 * keyed `fNNN`.
 */
export function parityScores(report, { scenario_key = null, godot_version = null } = {}) {
  const traces = {};
  for (const [id, d] of Object.entries(report.traces || {})) {
    traces[id] = { compared: d.compared ?? 0, ...traceScore(d, report.tolerance ?? 0.01) };
  }
  const images = {};
  for (const im of report.images || []) {
    images[`f${im.frame}`] = {
      mismatch_pct: round(im.mismatch * 100, 3),
      mean_delta: round(im.meanDelta, 3),
      coarse_mismatch_pct: round((im.coarseMismatch ?? 0) * 100, 3),
      side: im.side ?? null,
      godot: im.godot ?? null,
      patina: im.patina ?? null,
    };
  }
  return { scenario_key, godot_version: godot_version ?? report.godot?.version ?? null, traces, images };
}

/**
 * First divergence and max error of one diffTrace result. A frame where
 * NEITHER engine has the node (spark_keeper's /root/Main before the title
 * screen changes scene) agrees: diffTrace counts it as an infinite error,
 * which would pin max error at "unreadable" forever and hide every later
 * change. Only a frame where exactly one side is missing is infinite.
 */
export function traceScore(d, tolerance = 0.01) {
  if (!Array.isArray(d.series)) {
    return { first_divergence: d.firstDivergence ?? null, max_error: round(finiteOrNull(d.maxError)), max_error_frame: d.maxErrorFrame ?? null };
  }
  let first = null, max = 0, maxFrame = null;
  for (const s of d.series) {
    const bothAbsent = s.ref == null && s.act == null;
    const err = bothAbsent ? 0 : s.ref == null || s.act == null ? Infinity : typeof s.error === 'number' ? s.error : Infinity;
    if (err > tolerance && first === null) first = s.frame;
    if (maxFrame === null || err > max) { max = err; maxFrame = s.frame; }
  }
  return { first_divergence: first, max_error: round(finiteOrNull(max)), max_error_frame: maxFrame };
}

const fmtN = (x) => (x === null || x === undefined ? 'unreadable' : Math.abs(x) >= 100 ? x.toFixed(1) : Math.abs(x) >= 1 ? x.toFixed(3) : x.toPrecision(3));

/** One "lower is better" score against its baseline. Infinite (null) is the worst value. */
function lowerIsBetter(base, cur, abs, rel) {
  if (base === undefined) return null;
  if (base === null && cur === null) return { kind: 'same', severity: 0 };
  if (cur === null) return { kind: 'worse', severity: Infinity };
  if (base === null) return { kind: 'better', severity: Infinity };
  const slack = abs + rel * Math.abs(base);
  const d = cur - base;
  if (d > slack) return { kind: 'worse', severity: slack > 0 ? d / slack : Infinity };
  if (-d > slack) return { kind: 'better', severity: slack > 0 ? -d / slack : Infinity };
  return { kind: 'same', severity: 0 };
}

/**
 * Compares a game's current scores with its baseline. Returns
 * {status, regressions, improvements, stale, compared}:
 * - status 'no-baseline' (nothing recorded), 'stale' (recorded under another
 *   scenario or Godot version: not comparable, re-record), 'regressed' or 'ok';
 * - each regression/improvement is {metric, id, base, cur, severity, text},
 *   regressions sorted worst first (severity = how many noise margins).
 */
export function compareParity(base, cur, margins = {}) {
  const m = { ...DEFAULT_MARGINS, ...(base?.margins || {}), ...margins };
  const out = { status: 'ok', regressions: [], improvements: [], stale: [], compared: 0 };
  if (!base) return { ...out, status: 'no-baseline' };
  if (base.scenario_key && cur.scenario_key && base.scenario_key !== cur.scenario_key) out.stale.push(`scenario changed (baseline ${base.scenario_key}, now ${cur.scenario_key})`);
  if (base.godot_version && cur.godot_version && base.godot_version !== cur.godot_version) out.stale.push(`Godot version changed (baseline ${base.godot_version}, now ${cur.godot_version})`);
  if (out.stale.length) return { ...out, status: 'stale' };

  const push = (r, metric, id, b, c, unit, text) => {
    if (!r || r.kind === 'same') return;
    const e = { metric, id, base: b, cur: c, severity: r.severity, text: `${id} ${text} ${r.kind === 'worse' ? 'regressed' : 'improved'}: ${fmtN(b)}${unit} -> ${fmtN(c)}${unit}` };
    (r.kind === 'worse' ? out.regressions : out.improvements).push(e);
  };

  for (const [id, b] of Object.entries(base.traces || {})) {
    const c = cur.traces?.[id];
    if (!c) {
      out.regressions.push({ metric: 'trace', id, base: b.max_error, cur: null, severity: Infinity, text: `${id} is no longer traced` });
      continue;
    }
    out.compared++;
    push(lowerIsBetter(b.max_error, c.max_error, m.traceMaxErrorAbs, m.traceMaxErrorRel), 'trace_max_error', id, b.max_error, c.max_error, '', 'max error');
    // Divergence: later is better. No divergence (null) = the whole run matched.
    const span = Math.max(b.compared || 0, c.compared || 0) + 1;
    const bd = b.first_divergence ?? span;
    const cd = c.first_divergence ?? span;
    const slack = m.divergenceFrames;
    if (bd - cd > slack) out.regressions.push({ metric: 'trace_divergence', id, base: b.first_divergence, cur: c.first_divergence, severity: (bd - cd) / Math.max(slack, 1), text: `${id} first divergence regressed: ${b.first_divergence === null ? 'none' : `f${b.first_divergence}`} -> f${c.first_divergence}` });
    else if (cd - bd > slack) out.improvements.push({ metric: 'trace_divergence', id, base: b.first_divergence, cur: c.first_divergence, severity: (cd - bd) / Math.max(slack, 1), text: `${id} first divergence improved: f${b.first_divergence} -> ${c.first_divergence === null ? 'none' : `f${c.first_divergence}`}` });
  }
  for (const [id, b] of Object.entries(base.images || {})) {
    const c = cur.images?.[id];
    if (!c) {
      out.regressions.push({ metric: 'picture', id, base: b.mismatch_pct, cur: null, severity: Infinity, text: `picture ${id} is missing (Patina screenshot failed?)` });
      continue;
    }
    out.compared++;
    push(lowerIsBetter(b.mismatch_pct, c.mismatch_pct, m.mismatchPctAbs, m.mismatchPctRel), 'picture_mismatch', id, b.mismatch_pct, c.mismatch_pct, '%', 'mismatch');
    push(lowerIsBetter(b.mean_delta, c.mean_delta, m.meanDeltaAbs, m.meanDeltaRel), 'picture_mean_delta', id, b.mean_delta, c.mean_delta, '', 'mean RGB delta');
  }
  out.regressions.sort((a, b) => b.severity - a.severity);
  out.improvements.sort((a, b) => b.severity - a.severity);
  if (out.regressions.length) out.status = 'regressed';
  return out;
}

/** The baseline entry recorded for a game from this run's scores. */
export function baselineEntry(scores, { recorded, commit = null, margins = null } = {}) {
  const strip = Object.fromEntries(Object.entries(scores.images).map(([k, v]) => [k, { mismatch_pct: v.mismatch_pct, mean_delta: v.mean_delta, coarse_mismatch_pct: v.coarse_mismatch_pct }]));
  const e = { scenario_key: scores.scenario_key, godot_version: scores.godot_version, recorded, commit, traces: scores.traces, images: strip };
  if (margins) e.margins = margins;
  return e;
}

/** A stable bead title for a game's worst regression: names the metric, never the numbers (so it dedupes). */
export function regressionBeadTitle(game, reg) {
  const what = {
    trace_max_error: 'max error',
    trace_divergence: 'first divergence',
    trace: 'trace missing',
    picture_mismatch: 'picture mismatch',
    picture_mean_delta: 'picture mean delta',
    picture: 'picture missing',
  }[reg.metric] || reg.metric;
  return `[godot-parity] ${game}: ${reg.id} ${what} regressed vs Godot`.slice(0, 140);
}

// ---------------------------------------------------------------------------
// Report (.playtest-gate/parity.md)
// ---------------------------------------------------------------------------

function trend(base, cur, lowerBetter = true, slack = 0) {
  if (base === undefined) return 'new';
  if (base === null && cur === null) return 'same';
  if (cur === null) return lowerBetter ? 'WORSE' : 'better';
  if (base === null) return lowerBetter ? 'better' : 'WORSE';
  const d = cur - base;
  if (Math.abs(d) <= slack) return 'same';
  return (d > 0) === lowerBetter ? 'WORSE' : 'better';
}
const cell = (x, unit = '') => (x === undefined ? '-' : `${fmtN(x)}${x === null ? '' : unit}`);
const frameCell = (x) => (x === undefined ? '-' : x === null ? 'none' : `f${x}`);

/**
 * Renders the parity report. `results` are the gate's per-game results (each
 * with `.parity` = {scores, comparison, cache, out, error}); `baselines` the
 * parsed baselines file.
 */
export function renderParityReport({ at, results, baselines = {} }) {
  const lines = [`# Godot parity report`, '', `Run: ${at}. Baselines: scripts/godot-parity-baselines.json. Lower is better for every number; "WORSE" beyond the noise margin fails the gate.`, ''];
  const rows = results.filter((r) => r.parity);
  if (!rows.length) return lines.concat('No game ran with `--godot`.', '').join('\n');
  lines.push('| game | status | regressions | improvements | worst regression |', '|---|---|---|---|---|');
  for (const r of rows) {
    const p = r.parity;
    const c = p.comparison || {};
    const st = p.error ? 'error' : c.status || '-';
    lines.push(`| ${r.game} | ${st} | ${(c.regressions || []).length} | ${(c.improvements || []).length} | ${p.error ? p.error.split('\n')[0].slice(0, 120) : c.regressions?.[0]?.text || '-'} |`);
  }
  lines.push('');
  for (const r of rows) {
    const p = r.parity;
    const base = baselines[r.game];
    lines.push(`## ${r.game}`, '');
    if (p.error) {
      lines.push(`Could not compare: ${p.error}`, '');
      continue;
    }
    const c = p.comparison || {};
    const recorded = base ? `baseline recorded ${base.recorded || '?'}${base.commit ? ` at ${base.commit}` : ''}` : 'no baseline recorded';
    lines.push(`Status **${c.status}**, ${recorded}. Godot capture ${p.cache?.hit ? 'reused from cache' : 'captured'} (key ${p.cache?.key || '?'}). Out: \`${p.out || '?'}\``, '');
    for (const s of c.stale || []) lines.push(`- stale baseline: ${s}`);
    for (const x of c.regressions || []) lines.push(`- REGRESSION: ${x.text}`);
    for (const x of c.improvements || []) lines.push(`- improved: ${x.text}`);
    if ((c.stale || []).length + (c.regressions || []).length + (c.improvements || []).length) lines.push('');
    const s = p.scores || { traces: {}, images: {} };
    if (Object.keys(s.traces).length) {
      lines.push('| trace | first divergence (base -> now) | max error (base -> now) | trend |', '|---|---|---|---|');
      for (const [id, t] of Object.entries(s.traces)) {
        const b = base?.traces?.[id];
        lines.push(`| \`${id}\` | ${frameCell(b?.first_divergence)} -> ${frameCell(t.first_divergence)} | ${cell(b?.max_error)} -> ${cell(t.max_error)} | ${trend(b?.max_error, t.max_error, true, DEFAULT_MARGINS.traceMaxErrorAbs)} |`);
      }
      lines.push('');
    }
    if (Object.keys(s.images).length) {
      lines.push('| picture | mismatch (base -> now) | mean delta (base -> now) | trend | Godot \\| Patina \\| heat |', '|---|---|---|---|---|');
      for (const [id, im] of Object.entries(s.images)) {
        const b = base?.images?.[id];
        const t1 = trend(b?.mismatch_pct, im.mismatch_pct, true, DEFAULT_MARGINS.mismatchPctAbs);
        const t2 = trend(b?.mean_delta, im.mean_delta, true, DEFAULT_MARGINS.meanDeltaAbs);
        const t = t1 === 'WORSE' || t2 === 'WORSE' ? 'WORSE' : t1 === 'better' || t2 === 'better' ? 'better' : t1;
        lines.push(`| ${id} | ${cell(b?.mismatch_pct, '%')} -> ${cell(im.mismatch_pct, '%')} | ${cell(b?.mean_delta)} -> ${cell(im.mean_delta)} | ${t} | \`${im.side || '-'}\` |`);
      }
      lines.push('');
    }
  }
  return lines.join('\n');
}
