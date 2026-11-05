// Tests for the Godot-parity gate's pure logic (scripts/lib/godot-parity.mjs,
// scripts/parity-report.mjs): `node --test scripts/tests/godot-parity.test.mjs`.
//
// The gate's one promise is "parity can only improve": a score that got worse
// beyond its noise margin fails, one inside the margin does not, and a
// baseline recorded under another scenario is never compared frame by frame.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  DEFAULT_MARGINS, hashGameFiles, scenarioKey, captureCacheKey, captureComplete,
  parityScores, compareParity, baselineEntry, regressionBeadTitle, renderParityReport,
} from '../lib/godot-parity.mjs';
import { readBaselines, writeParityReport } from '../parity-report.mjs';

const scenario = (over = {}) => ({
  scene: 'res://pong.tscn', keys: 'wait:800,KeyW:1000', fps: 60, frames: 108, size: [640, 400], shots: [48, 108],
  watches: [{ id: '/root/Pong/Left:position', path: '/root/Pong/Left', prop: 'position' }], ...over,
});

/** A compare-to-godot report.json, as far as the scores read it. */
const report = ({ maxError = 0.5, firstDivergence = 40, mismatch = 0.1, meanDelta = 20 } = {}) => ({
  godot: { version: '4.6.1-stable (official)' },
  traces: { '/root/Pong/Left:position': { compared: 108, firstDivergence, maxError, maxErrorFrame: 100 } },
  images: [
    { frame: 48, mismatch, meanDelta, coarseMismatch: mismatch / 2, side: '/x/diff/f00048-side.png', godot: '/x/g.png', patina: '/x/p.png' },
    { frame: 108, mismatch: 0.05, meanDelta: 10, coarseMismatch: 0.01, side: '/x/diff/f00108-side.png' },
  ],
});
const scores = (o) => parityScores(report(o), { scenario_key: 'k1', godot_version: '4.6.1' });
const base = (o) => baselineEntry(scores(o), { recorded: '2026-11-05', commit: 'abc' });

// --- scores -----------------------------------------------------------------

test('scores keep divergence, max error, mismatch in percent and mean delta per picture', () => {
  const s = scores();
  assert.deepEqual(s.traces['/root/Pong/Left:position'], { compared: 108, first_divergence: 40, max_error: 0.5, max_error_frame: 100 });
  assert.equal(s.images.f48.mismatch_pct, 10);
  assert.equal(s.images.f48.mean_delta, 20);
  assert.equal(s.images.f48.coarse_mismatch_pct, 5);
  assert.equal(s.images.f48.side, '/x/diff/f00048-side.png');
  assert.equal(s.scenario_key, 'k1');
});

test('an infinite max error (a side could not read the property) is stored as null, which JSON keeps', () => {
  const s = scores({ maxError: Infinity });
  assert.equal(s.traces['/root/Pong/Left:position'].max_error, null);
  assert.equal(JSON.parse(JSON.stringify(s)).traces['/root/Pong/Left:position'].max_error, null);
});

test('frames where NEITHER engine has the node agree; one side missing is infinite', () => {
  // diffTrace reports Infinity for the both-absent frames (the title screen
  // before /root/Main exists), which would hide the real score for good.
  const r = report();
  r.traces['/root/Pong/Left:position'] = {
    compared: 4, firstDivergence: 0, maxError: Infinity, maxErrorFrame: 0,
    series: [
      { frame: 0, ref: null, act: null, error: Infinity },
      { frame: 1, ref: [1, 1], act: [1, 1], error: 0 },
      { frame: 2, ref: [1, 1], act: [1, 1.5], error: 0.5 },
      { frame: 3, ref: [1, 1], act: [1, 1.25], error: 0.25 },
    ],
  };
  const t = parityScores(r).traces['/root/Pong/Left:position'];
  assert.deepEqual(t, { compared: 4, first_divergence: 2, max_error: 0.5, max_error_frame: 2 });
  r.traces['/root/Pong/Left:position'].series[3] = { frame: 3, ref: [1, 1], act: null, error: Infinity };
  assert.equal(parityScores(r).traces['/root/Pong/Left:position'].max_error, null);
});

test('a baseline entry drops the per-run PNG paths', () => {
  const b = base();
  assert.deepEqual(b.images.f48, { mismatch_pct: 10, mean_delta: 20, coarse_mismatch_pct: 5 });
  assert.equal(b.commit, 'abc');
});

// --- comparison and noise margins ---------------------------------------------

test('identical scores are ok, with nothing worse or better', () => {
  const c = compareParity(base(), scores());
  assert.equal(c.status, 'ok');
  assert.deepEqual([c.regressions.length, c.improvements.length], [0, 0]);
  assert.equal(c.compared, 3);
});

test('a max error just inside abs + rel*base is noise, just outside is a regression', () => {
  const slack = DEFAULT_MARGINS.traceMaxErrorAbs + DEFAULT_MARGINS.traceMaxErrorRel * 0.5; // 0.1
  assert.equal(compareParity(base(), scores({ maxError: 0.5 + slack - 0.001 })).status, 'ok');
  const c = compareParity(base(), scores({ maxError: 0.5 + slack + 0.01 }));
  assert.equal(c.status, 'regressed');
  assert.equal(c.regressions[0].metric, 'trace_max_error');
  assert.match(c.regressions[0].text, /max error regressed: 0\.500 -> 0\.610/);
});

test('a lower max error beyond the margin is an improvement, never a failure', () => {
  const c = compareParity(base({ maxError: 5 }), scores({ maxError: 1 }));
  assert.equal(c.status, 'ok');
  assert.equal(c.improvements[0].metric, 'trace_max_error');
});

test('an EARLIER first divergence beyond the frame margin regresses; a later one improves', () => {
  const m = DEFAULT_MARGINS.divergenceFrames;
  assert.equal(compareParity(base(), scores({ firstDivergence: 40 - m })).status, 'ok');
  const worse = compareParity(base(), scores({ firstDivergence: 40 - m - 1 }));
  assert.equal(worse.regressions[0].metric, 'trace_divergence');
  const better = compareParity(base(), scores({ firstDivergence: 80 }));
  assert.equal(better.status, 'ok');
  assert.equal(better.improvements[0].metric, 'trace_divergence');
});

test('going from no divergence at all to any divergence is a regression', () => {
  const c = compareParity(base({ firstDivergence: null }), scores({ firstDivergence: 100 }));
  assert.equal(c.regressions[0].metric, 'trace_divergence');
  assert.match(c.regressions[0].text, /none -> f100/);
});

test('a property that turned unreadable is the worst regression there is', () => {
  const c = compareParity(base(), scores({ maxError: Infinity, mismatch: 0.5 }));
  assert.equal(c.regressions[0].metric, 'trace_max_error');
  assert.equal(c.regressions[0].severity, Infinity);
});

test('picture mismatch and mean delta each have their own margin', () => {
  // +1.9 percentage points mismatch is noise; +2.1 is not.
  assert.equal(compareParity(base(), scores({ mismatch: 0.119 })).status, 'ok');
  assert.equal(compareParity(base(), scores({ mismatch: 0.121 })).regressions[0].metric, 'picture_mismatch');
  // mean delta 20: slack 2 + 5% of 20 = 3.
  assert.equal(compareParity(base(), scores({ meanDelta: 22.9 })).status, 'ok');
  assert.equal(compareParity(base(), scores({ meanDelta: 23.1 })).regressions[0].metric, 'picture_mean_delta');
});

test('regressions are sorted worst first by how many margins they exceed', () => {
  // The trace is checked (and pushed) first, but 1.5 margins over is milder
  // than the picture's 20 margins over, so the picture leads (it is the bead).
  const c = compareParity(base(), scores({ maxError: 0.65, mismatch: 0.5 }));
  assert.deepEqual(c.regressions.map((r) => r.metric), ['picture_mismatch', 'trace_max_error']);
});

test('a per-game margin in the baseline file overrides the default', () => {
  const b = { ...base(), margins: { meanDeltaAbs: 10 } };
  assert.equal(compareParity(b, scores({ meanDelta: 29 })).status, 'ok');
  assert.equal(compareParity(b, scores({ meanDelta: 32 })).status, 'regressed');
});

test('a picture the baseline has and the run lacks is a regression', () => {
  const s = scores();
  delete s.images.f108;
  const c = compareParity(base(), s);
  assert.equal(c.regressions[0].metric, 'picture');
  assert.match(c.regressions[0].text, /picture f108 is missing/);
});

test('no baseline, or one recorded under another scenario or Godot, is never compared', () => {
  assert.equal(compareParity(undefined, scores()).status, 'no-baseline');
  const other = { ...scores({ mismatch: 0.9 }), scenario_key: 'k2' };
  const c = compareParity(base(), other);
  assert.equal(c.status, 'stale');
  assert.equal(c.regressions.length, 0);
  assert.match(c.stale[0], /scenario changed/);
  assert.match(compareParity(base(), { ...scores(), godot_version: '4.7' }).stale[0], /Godot version changed/);
});

test('the bead title names the metric, not the numbers, so a re-run dedupes onto it', () => {
  const a = compareParity(base(), scores({ meanDelta: 40 })).regressions[0];
  const b = compareParity(base(), scores({ meanDelta: 60 })).regressions[0];
  assert.equal(regressionBeadTitle('pong', a), regressionBeadTitle('pong', b));
  assert.equal(regressionBeadTitle('pong', a), '[godot-parity] pong: f48 picture mean delta regressed vs Godot');
});

// --- cache key ------------------------------------------------------------------

function tmpGame() {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), 'parity-game-'));
  fs.writeFileSync(path.join(d, 'project.godot'), 'config_version=5\n');
  fs.mkdirSync(path.join(d, 'scenes'));
  fs.writeFileSync(path.join(d, 'scenes', 'main.tscn'), '[gd_scene]\n');
  return d;
}

test('the cache key is stable, and changes with a game file, the scenario, Godot or the tool', () => {
  const d = tmpGame();
  const key = (o = {}) => captureCacheKey({ files: hashGameFiles(d), scenario: scenario(), godotVersion: '4.6.1', tool: 't', ...o });
  const k = key();
  assert.match(k, /^[0-9a-f]{16}$/);
  assert.equal(key(), k);
  assert.notEqual(key({ scenario: scenario({ keys: 'wait:800,KeyW:1100' }) }), k);
  assert.notEqual(key({ scenario: scenario({ shots: [48] }) }), k);
  assert.notEqual(key({ godotVersion: '4.6.2' }), k);
  assert.notEqual(key({ tool: 'u' }), k);
  fs.writeFileSync(path.join(d, 'scenes', 'main.tscn'), '[gd_scene format=3]\n');
  assert.notEqual(key(), k);
});

test('the import cache (.godot/) and the editor state (.editor/) do not change the key; a new file does', () => {
  const d = tmpGame();
  const key = () => captureCacheKey({ files: hashGameFiles(d), scenario: scenario(), godotVersion: '4.6.1' });
  const k = key();
  fs.mkdirSync(path.join(d, '.godot', 'imported'), { recursive: true });
  fs.writeFileSync(path.join(d, '.godot', 'imported', 'x.ctex'), 'blob');
  assert.equal(key(), k);
  // The Patina editor writes .editor/ into the game on every start.
  fs.mkdirSync(path.join(d, '.editor'));
  fs.writeFileSync(path.join(d, '.editor', 'layout.json'), '{}');
  assert.equal(key(), k);
  fs.writeFileSync(path.join(d, 'icon.svg'), '<svg/>');
  assert.notEqual(key(), k);
});

test('the scenario key ignores the game files but not the watches', () => {
  assert.equal(scenarioKey(scenario()), scenarioKey(scenario()));
  assert.notEqual(scenarioKey(scenario()), scenarioKey(scenario({ watches: [] })));
});

test('a cached capture counts only when every recorded shot is there', () => {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), 'parity-cap-'));
  assert.equal(captureComplete(d), false);
  fs.mkdirSync(path.join(d, 'frames'));
  fs.writeFileSync(path.join(d, 'trace.json'), JSON.stringify({ meta: { shots: [48, 108] } }));
  fs.writeFileSync(path.join(d, 'frames', 'f00048.png'), 'x');
  assert.equal(captureComplete(d), false);
  fs.writeFileSync(path.join(d, 'frames', 'f00108.png'), 'x');
  assert.equal(captureComplete(d), true);
});

// --- report -----------------------------------------------------------------------

const run = (s, c, extra = {}) => ({ game: 'pong', parity: { scores: s, comparison: c, cache: { key: 'abcd', hit: true }, out: '/o/pong', ...extra } });

test('the report shows base -> now per score, the trend and the side-by-side path', () => {
  const b = base();
  const s = scores({ meanDelta: 40 });
  const md = renderParityReport({ at: 'T', results: [run(s, compareParity(b, s))], baselines: { pong: b } });
  assert.match(md, /\| pong \| regressed \| 1 \| 0 \| f48 mean RGB delta regressed: 20\.000 -> 40\.000 \|/);
  assert.match(md, /\| f48 \| 10\.000% -> 10\.000% \| 20\.000 -> 40\.000 \| WORSE \| `\/x\/diff\/f00048-side\.png` \|/);
  assert.match(md, /\| f108 \| 5\.000% -> 5\.000% \| 10\.000 -> 10\.000 \| same \|/);
  assert.match(md, /\| `\/root\/Pong\/Left:position` \| f40 -> f40 \| 0\.500 -> 0\.500 \| same \|/);
  assert.match(md, /REGRESSION: f48 mean RGB delta regressed/);
  assert.match(md, /reused from cache \(key abcd\)/);
});

test('an improvement reads "better", a game with no baseline "new", an error is shown, not dropped', () => {
  const b = base({ meanDelta: 40 });
  const s = scores();
  const md = renderParityReport({
    at: 'T',
    results: [run(s, compareParity(b, s)), { game: 'spark', parity: { scores: s, comparison: compareParity(undefined, s), cache: {} } }, { game: 'p3d', parity: { error: 'Godot capture failed: boom' } }, { game: 'nogodot' }],
    baselines: { pong: b },
  });
  assert.match(md, /\| f48 \| 10\.000% -> 10\.000% \| 40\.000 -> 20\.000 \| better \|/);
  assert.match(md, /## spark[\s\S]*\| f48 \| - -> 10\.000% \| - -> 20\.000 \| new \|/);
  assert.match(md, /\| p3d \| error \| 0 \| 0 \| Godot capture failed: boom \|/);
  assert.doesNotMatch(md, /nogodot/);
});

test('writeParityReport writes the file and readBaselines reads the games map', () => {
  const d = fs.mkdtempSync(path.join(os.tmpdir(), 'parity-md-'));
  const bf = path.join(d, 'b.json');
  fs.writeFileSync(bf, JSON.stringify({ _doc: 'x', games: { pong: base() } }));
  const baselines = readBaselines(bf);
  assert.deepEqual(Object.keys(baselines), ['pong']);
  const s = scores();
  const out = path.join(d, 'parity.md');
  writeParityReport({ latest: { at: 'T', results: [run(s, compareParity(baselines.pong, s))] }, baselines, out });
  assert.match(fs.readFileSync(out, 'utf8'), /^# Godot parity report/);
  assert.deepEqual(readBaselines(path.join(d, 'missing.json')), {});
});
