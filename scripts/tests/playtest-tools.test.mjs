// Tests for the playtest tools themselves: `node --test scripts/tests/`.
//
// browser-playtest once sat silent for 6.5 hours (its profiler's exit event
// fired before anyone listened, and no DevTools call had a deadline), and the
// gate's verdicts decide which beads get filed — both deserve tests.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import crypto from 'node:crypto';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import fs from 'node:fs';
import os from 'node:os';
import { checkExpect, judge, validateScenario, loadScenarios } from '../playtest-gate.mjs';
import { editorAxisLines, watchVerdict } from '../lib/playtest-checks.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const PLAYTEST = path.join(ROOT, 'scripts', 'browser-playtest.mjs');

function listen(server) {
  return new Promise((r) => server.listen(0, '127.0.0.1', () => r(server.address().port)));
}

/// An editor that answers the status probe and nothing interesting.
async function fakeEditor() {
  let running = false;
  const server = http.createServer((req, res) => {
    const json = (o) => { res.setHeader('Content-Type', 'application/json'); res.end(JSON.stringify(o)); };
    if (req.url.startsWith('/api/runtime/status')) return json({ running, build: 'test', render_ms: 1, sim_fps: 60 });
    if (req.url.startsWith('/api/runtime/play')) { running = true; return json({ ok: true }); }
    if (req.url.startsWith('/api/runtime/stop')) { running = false; return json({ ok: true }); }
    if (req.url.startsWith('/api/logs')) return json([]);
    if (req.url.startsWith('/mcp')) return json({ result: { content: [{ type: 'text', text: 'build: test' }] } });
    if (req.url.startsWith('/editor')) {
      res.setHeader('Content-Type', 'text/html');
      return res.end(`<!doctype html><button id="btn-play" onclick="fetch('/api/runtime/play',{method:'POST'})">Play</button>`);
    }
    res.statusCode = 404; res.end('{}');
  });
  return { server, port: await listen(server) };
}

/// A DevTools endpoint that opens a tab and then never answers a single call.
async function silentChrome() {
  const server = http.createServer((req, res) => {
    const port = server.address().port;
    res.setHeader('Content-Type', 'application/json');
    res.end(JSON.stringify({ id: 'T1', webSocketDebuggerUrl: `ws://127.0.0.1:${port}/devtools/page/T1` }));
  });
  server.on('upgrade', (req, socket) => {
    const accept = crypto.createHash('sha1').update(req.headers['sec-websocket-key'] + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
    socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
    socket.on('data', () => {}); // swallow every command, answer none
  });
  return { server, port: await listen(server) };
}

function run(args, timeoutMs) {
  return new Promise((resolve) => {
    const t0 = Date.now();
    const p = spawn(process.execPath, [PLAYTEST, ...args], { stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '', err = '';
    p.stdout.on('data', (d) => (out += d));
    p.stderr.on('data', (d) => (err += d));
    const killer = setTimeout(() => p.kill('SIGKILL'), timeoutMs);
    p.on('exit', (code, signal) => { clearTimeout(killer); resolve({ code, signal, out, err, ms: Date.now() - t0 }); });
  });
}

test('a Chrome that never answers ends the run within its budget, exit 2', async () => {
  const ed = await fakeEditor();
  const cdp = await silentChrome();
  try {
    const r = await run(['--url', `http://127.0.0.1:${ed.port}/editor`, '--cdp', String(cdp.port), '--keys', 'wait:100', '--budget-ms', '4000'], 30000);
    assert.notEqual(r.signal, 'SIGKILL', `the run hung until the test killed it (${r.ms} ms)`);
    assert.equal(r.code, 2, r.err);
    assert.match(r.err, /giving up/);
    assert.ok(r.ms < 15000, `took ${r.ms} ms`);
  } finally {
    ed.server.close(); cdp.server.close();
  }
});

test('a profile shorter than the key steps still finishes (sample exit not missed)', { skip: process.platform !== 'darwin' && 'needs macOS sample' }, async () => {
  const ed = await fakeEditor();
  try {
    // 1 s of profile, 3 s of steps: `sample` exits long before the steps end.
    // The editor pid is this test process (it serves the fake editor).
    const r = await run(['--url', `http://127.0.0.1:${ed.port}/editor`, '--keys', 'wait:3000', '--profile', '1', '--json', '--min-fps', '0'], 120000);
    assert.notEqual(r.signal, 'SIGKILL', `the run hung (${r.ms} ms): ${r.err.slice(-300)}`);
    assert.ok(r.code === 0 || r.code === 1, `exit ${r.code}: ${r.err.slice(-300)}`);
    const report = JSON.parse(r.out);
    assert.ok(report.profile_file, 'the profile was collected');
  } finally {
    ed.server.close();
  }
});

// --- the gate's judgement -----------------------------------------------

const report = (over = {}) => ({
  steps: [{ step: 'wait:500' }, { step: 'ArrowRight:1000' }, { step: 'ArrowUp:500' }],
  watch: [{ watch: '/root/P:position', values: [
    { after: 'wait:500', value: '[10,50]' }, { after: 'ArrowRight:1000', value: '[90,50]' }, { after: 'ArrowUp:500', value: '[90,50]' },
  ] }],
  problems: [], warnings: [], server: { render_ms: 10 },
  ...over,
});

test('expect: increase/decrease/change across one step, per axis', () => {
  const r = report();
  assert.equal(checkExpect(r, { watch: '/root/P:position', after: 'ArrowRight:1000', axis: 0, change: 'increase' }), null);
  assert.match(checkExpect(r, { watch: '/root/P:position', after: 'ArrowUp:500', axis: 1, change: 'decrease' }), /should decrease across ArrowUp:500 but went 50 → 50/);
  assert.match(checkExpect(r, { watch: '/root/P:position', after: 'ArrowUp:500', change: 'change' }), /should change/);
  assert.match(checkExpect(r, { watch: '/root/P:position', after: 'Space:1', change: 'change' }), /no step "Space:1"/);
  const unreadable = report({ watch: [{ watch: '/root/P:position', values: [{ after: 'wait:500', value: '(no /root/P in the running scene)' }, { after: 'ArrowRight:1000', value: '[1,1]' }] }] });
  assert.match(checkExpect(unreadable, { watch: '/root/P:position', after: 'ArrowRight:1000', change: 'increase' }), /not readable/);
});

test('expect pairs values by step index, so a repeated step label is fine', () => {
  // spark_keeper: wait, Enter (scene changes), wait, ArrowRight — two "wait:800".
  const r = {
    steps: ['wait:800', 'Enter:300', 'wait:800', 'ArrowRight:1000'].map((step) => ({ step })),
    watch: [{ watch: '/root/Main/Player:position', values: [
      { after: 'wait:800', value: '(no position: node not found)' }, { after: 'Enter:300', value: '[320,180]' },
      { after: 'wait:800', value: '[320,180]' }, { after: 'ArrowRight:1000', value: '[531.8,180]' },
    ] }],
  };
  assert.equal(checkExpect(r, { watch: '/root/Main/Player:position', after: 'ArrowRight:1000', axis: 0, change: 'increase' }), null);
});

test('judge splits functional from perf, honours allow_findings and the baseline', () => {
  const sc = { expect: [], max_audio_underruns: 0 };
  const r = report({
    problems: ['shown 30 frames/s, below --min-fps 45', '2 editor API request(s) failed: 404 GET /api/preview/file', 'game error ×3: boom'],
    warnings: ['editor: render-bound: 40 of 60 game frames rendered', 'the FPS counter is not visible over the game'],
    audio: 'device: 48000 Hz x2 · callbacks 600 · underruns 4 · priming waits 2',
    server: { render_ms: 20 },
  });
  const v = judge(sc, r, { render_ms: 10 });
  assert.deepEqual(v.functional, ['2 editor API request(s) failed: 404 GET /api/preview/file', 'game error ×3: boom']);
  assert.ok(v.perf.includes('shown 30 frames/s, below --min-fps 45'));
  assert.ok(v.perf.some((p) => p.startsWith('editor: render-bound')));
  assert.ok(v.perf.includes('audio: 4 underruns (max 0)'));
  assert.ok(v.perf.some((p) => /over baseline 10 ms/.test(p)));
  // Allowed findings and in-budget numbers are not failures.
  const ok = judge({ ...sc, allow_findings: ['render-bound'], max_audio_underruns: 5 }, { ...r, problems: [] }, { render_ms: 18 });
  assert.deepEqual(ok, { functional: [], perf: [] });
});

// --- scenarios, watched values, editor overlays (pat-iawbc) ------------------

test('every checked-in scenario validates, platformer_3d included', () => {
  const all = loadScenarios(null);
  assert.ok(all.some((s) => s.game === 'platformer_3d'), 'games/platformer_3d/playtest.json is loaded');
  for (const sc of all) assert.equal(sc.invalid, undefined, `${sc.game}: ${sc.invalid}`);
});

test('validateScenario names what is wrong instead of letting the gate crash', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'gate-scenario-'));
  try {
    fs.writeFileSync(path.join(dir, 'main.tscn'), '[gd_scene format=3]\n');
    const ok = { dir, scene: 'main.tscn', keys: 'wait:500,KeyW:1000,down:ShiftLeft,up:ShiftLeft', watch: ['/root/P:position'],
      expect: [{ watch: '/root/P:position', after: 'KeyW:1000', change: 'change' }] };
    assert.deepEqual(validateScenario(ok), []);
    const bad = validateScenario({ ...ok, scene: 'nope.tscn', keys: 'wait:500,KeyW-1000',
      expect: [{ watch: '/root/Q:position', after: 'Space:300', change: 'grow' }] });
    assert.ok(bad.includes('scene nope.tscn does not exist'), bad.join('; '));
    assert.ok(bad.some((b) => b.includes('"KeyW-1000"')), bad.join('; '));
    assert.ok(bad.some((b) => b.includes('after "Space:300"')), bad.join('; '));
    assert.ok(bad.some((b) => b.includes('watch "/root/Q:position"')), bad.join('; '));
    assert.ok(bad.some((b) => b.includes('change "grow"')), bad.join('; '));
    // A playtest.json that does not parse is that game's error, not a throw.
    fs.mkdirSync(path.join(dir, 'broken'));
    fs.writeFileSync(path.join(dir, 'broken', 'playtest.json'), '{ "scene": ');
    const [sc] = loadScenarios(null, dir);
    assert.equal(sc.game, 'broken');
    assert.match(sc.invalid[0], /does not parse/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('a watched value that never moved: warning normally, problem when the scenario expects movement', () => {
  const still = [{ watch: '/root/Game/Player:position', values: [
    { after: 'wait:1500', value: '[1,2,3]' }, { after: 'KeyW:1500', value: '[1,2,3]' }] }];
  const never = [{ watch: '/root/Game/Player:position', values: [{ after: 'KeyW:1500', value: '(no position: not on /root/Game/Player, a Node)' }] }];
  const moved = [{ watch: '/root/Game/Player:position', values: [
    { after: 'wait:1500', value: '[1,2,3]' }, { after: 'KeyW:1500', value: '[1,2,4]' }] }];
  assert.deepEqual(watchVerdict(still, false).problems, []);
  assert.equal(watchVerdict(still, false).warnings.length, 1);
  assert.match(watchVerdict(still, true).problems[0], /stayed \[1,2,3\] through every step/);
  assert.match(watchVerdict(never, true).problems[0], /never readable: \(no position: not on \/root\/Game\/Player, a Node\)/);
  assert.deepEqual(watchVerdict(moved, true), { problems: [], warnings: [] });
});

test('the gate passes --expect-move for a scenario with expect rules, and an unreadable value names itself', () => {
  const src = fs.readFileSync(path.join(ROOT, 'scripts', 'playtest-gate.mjs'), 'utf8');
  assert.match(src, /if \(\(sc\.expect \|\| \[\]\)\.length\) args\.push\('--expect-move'\)/);
  const r = report({ watch: [{ watch: '/root/P:position', values: [
    { after: 'wait:500', value: '(no position: not on /root/P, a Node)' }, { after: 'ArrowRight:1000', value: '(no position: not on /root/P, a Node)' }] }] });
  assert.match(checkExpect(r, { watch: '/root/P:position', after: 'ArrowRight:1000', change: 'change' }), /not readable around ArrowRight:1000: \(no position: not on \/root\/P, a Node\)/);
  // The overlay and never-moved problems are FUNCTIONAL failures, not perf.
  const v = judge({ expect: [] }, report({ problems: ['editor overlay in the Play view: x', '/root/P:position stayed [1,1] through every step: do the controls reach the game?'] }), null);
  assert.equal(v.functional.length, 2);
  assert.deepEqual(v.perf, []);
});

/// A w x h RGBA frame of the 3D editor backdrop, with `draw(set)` on top.
function frame(w, h, draw) {
  const d = new Uint8Array(w * h * 4);
  for (let i = 0; i < d.length; i += 4) { d[i] = 62; d[i + 1] = 51; d[i + 2] = 40; d[i + 3] = 255; }
  const set = (x, y, [r, g, b]) => {
    if (x < 0 || y < 0 || x >= w || y >= h) return;
    const i = (y * w + x) * 4; d[i] = r; d[i + 1] = g; d[i + 2] = b;
  };
  draw(set);
  return d;
}
const AXIS_RED = [160, 70, 70]; // measured on platformer_3d's Play frame
const AXIS_BLUE = [84, 140, 224];

test('editor axis lines: a long oblique thin red or blue line is the editor overlay', () => {
  const W = 400, H = 300;
  // The X axis as platformer_3d's camera sees it: ~18 degrees off horizontal.
  const red = editorAxisLines(frame(W, H, (set) => { for (let x = 0; x < W; x++) set(x, Math.round(60 + x * 0.33), AXIS_RED); }), W, H);
  assert.equal(red.overlay, true, JSON.stringify(red));
  const blue = editorAxisLines(frame(W, H, (set) => { for (let x = 40; x < 300; x++) set(x, Math.round(280 - x * 0.9), AXIS_BLUE); }), W, H);
  assert.equal(blue.overlay, true, JSON.stringify(blue));
});

test('editor axis lines: no false positive on 2D art — borders, filled sprites, scattered dashes', () => {
  const W = 400, H = 300;
  // spark_keeper's Line2D border: long, thin, blue-violet — but axis-aligned.
  const border = editorAxisLines(frame(W, H, (set) => {
    for (let x = 20; x < 380; x++) { set(x, 20, [90, 80, 200]); set(x, 280, [90, 80, 200]); }
    for (let y = 20; y < 280; y++) { set(20, y, [90, 80, 200]); set(380, y, [90, 80, 200]); }
  }), W, H);
  assert.equal(border.overlay, false, JSON.stringify(border));
  // A big red sprite: red, but not thin.
  const blob = editorAxisLines(frame(W, H, (set) => { for (let y = 50; y < 250; y++) for (let x = 50; x < 350; x++) set(x, y, [220, 40, 40]); }), W, H);
  assert.equal(blob.overlay, false, JSON.stringify(blob));
  // Many short oblique red strokes (bricks, foliage): thin, but no ONE line holds a share.
  const dashes = editorAxisLines(frame(W, H, (set) => {
    for (let k = 0; k < 60; k++) { const ox = (k * 53) % 380, oy = (k * 37) % 280; for (let t = 0; t < 12; t++) set(ox + t, oy + Math.round(t * 0.5), AXIS_RED); }
  }), W, H);
  assert.equal(dashes.overlay, false, JSON.stringify(dashes));
  // A short oblique line: real, but too short to be an axis across the view.
  const short = editorAxisLines(frame(W, H, (set) => { for (let x = 100; x < 180; x++) set(x, Math.round(100 + x * 0.33), AXIS_RED); }), W, H);
  assert.equal(short.overlay, false, JSON.stringify(short));
  // A hatched texture: twenty long oblique strokes — each one long enough, but
  // an editor axis is ONE line holding most of the colour, not one of many.
  const hatch = editorAxisLines(frame(W, H, (set) => {
    for (let k = 0; k < 20; k++) for (let x = 0; x < 260; x++) set(x + k * 7, Math.round(10 + k * 14 + x * 0.3), AXIS_RED);
  }), W, H);
  assert.equal(hatch.overlay, false, JSON.stringify(hatch));
  // A 6 px wide oblique red bar (a laser, a ramp): long and straight, but not
  // a 1-2 px line, so not the editor axis.
  const bar = editorAxisLines(frame(W, H, (set) => {
    for (let x = 0; x < W; x++) for (let o = 0; o < 6; o++) set(x, Math.round(60 + x * 0.33) + o, AXIS_RED);
  }), W, H);
  assert.equal(bar.overlay, false, JSON.stringify(bar));
});
