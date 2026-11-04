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
import { checkExpect, judge } from '../playtest-gate.mjs';

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
