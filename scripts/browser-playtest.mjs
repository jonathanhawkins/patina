#!/usr/bin/env node
// Play a game in the Patina web editor the way a person does: in a real
// (headless) Chrome, through the editor page. Reports what the PLAYER gets:
// frames shown per second, page errors, refused requests, whether the game
// fills the view, the FPS counter, and the editor's own `diagnose` findings.
//
//   scripts/browser-playtest.mjs [options]
//
//   --url <editor url>    default http://localhost:$PATINA_PORT/editor (port 8080)
//   --keys <steps>        what to do once Play starts (default "wait:3000"),
//                         comma-separated:
//                           Code:ms    hold a key for ms, with real auto-repeat
//                                      (KeyD:1500, Space:100, ArrowUp:800)
//                           down:Code  press and never release: a keyup the OS
//                                      swallowed (down:MetaLeft for ⌘⌃⇧4)
//                           up:Code    release it
//                           wait:ms    do nothing
//                         Codes: KeyA-KeyZ, Digit0-Digit9, Space, Enter, Escape,
//                         Tab, Backspace, ArrowLeft/Right/Up/Down, ShiftLeft,
//                         ControlLeft, AltLeft, MetaLeft (Shift, Control, Alt,
//                         Meta also accepted).
//   --audio <secs>        measure what the audio DEVICE played over the last
//                         <secs> (<=10) before stopping: underruns, clicks, gaps,
//                         clipping (MCP `audio`), and save it as audio.wav
//   --watch <node:prop>   read a property of the running game after every step,
//                         e.g. /root/Main/Player:position, and warn if it never
//                         changed (controls that do nothing); repeatable.
//                         `<node>:#children` reads how many children the node
//                         has — spawned mobs, bullets, pickups — so a cleanup
//                         (`call_group(..., "queue_free")`) shows as a drop
//   --min-fps <n>         frames shown per second below this is a problem (45)
//   --profile <seconds>   also sample the editor process while playing and list
//                         its hottest functions (macOS `sample`)
//   --edit-steps <spec>   actions in the EDIT view before Play, comma-separated:
//                         `select:<node name>` clicks that scene-tree row,
//                         `wait:<ms>`. Every image the page then references is
//                         fetched (e.g. tile palette thumbnails) — a broken one
//                         is a problem
//   --soak <seconds>      keep playing that long after the steps, sampling the
//                         editor's render time and memory every 10 s; steady
//                         growth in either is a problem
//   --budget-ms <ms>      give up (exit 2, Play stopped) after this long
//                         (default: 2 min + the steps + soak + 4x the profile)
//   --out <dir>           screenshots and report.json (default: a temp dir)
//   --cdp <port>          drive an already running Chrome's DevTools port
//   --keep-running        leave the game running afterwards
//   --json                print the report as JSON
//
// Unless --cdp is given it starts its own headless Chrome with a throwaway
// profile on a free port, and kills only that Chrome: your own browser is
// never touched. Needs Node 22+ (built-in WebSocket); no npm packages.
//
// Exit status: 0 no problems · 1 problems found · 2 could not run.
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The comment block at the top of this file.
function usage() {
  const lines = fs.readFileSync(new URL(import.meta.url), 'utf8').split('\n').slice(1);
  const end = lines.findIndex((l) => !l.startsWith('//'));
  return lines.slice(0, end).map((l) => l.replace(/^\/\/ ?/, '')).join('\n');
}

function parseArgs(argv) {
  const opts = {
    url: `http://localhost:${process.env.PATINA_PORT || 8080}/editor`,
    keys: 'wait:3000',
    minFps: 45,
    profile: 0,
    out: null,
    cdp: null,
    keepRunning: false,
    audio: 0,
    json: false,
    watch: [],
    editSteps: '',
    soak: 0,
    budgetMs: 0,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => {
      if (i + 1 >= argv.length) bail(`${a} needs a value`);
      return argv[++i];
    };
    if (a === '--url') opts.url = value();
    else if (a === '--keys') opts.keys = value();
    else if (a === '--watch') opts.watch.push(value());
    else if (a === '--min-fps') opts.minFps = Number(value());
    else if (a === '--profile') opts.profile = Number(value());
    else if (a === '--out') opts.out = value();
    else if (a === '--cdp') opts.cdp = value();
    else if (a === '--keep-running') opts.keepRunning = true;
    else if (a === '--audio') opts.audio = Number(value());
    else if (a === '--json') opts.json = true;
    else if (a === '--edit-steps') opts.editSteps = value();
    else if (a === '--soak') opts.soak = Number(value());
    else if (a === '--budget-ms') opts.budgetMs = Number(value());
    else if (a === '-h' || a === '--help') {
      console.log(usage());
      process.exit(0);
    } else bail(`unknown option ${a} (see --help)`);
  }
  return opts;
}

function bail(message) {
  console.error(`browser-playtest: ${message}`);
  process.exit(2);
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

// code -> [key, windowsVirtualKeyCode, text, modifier bit, location]
const NAMED = {
  Space: [' ', 32, ' '],
  Enter: ['Enter', 13, '\r'],
  Escape: ['Escape', 27],
  Tab: ['Tab', 9],
  Backspace: ['Backspace', 8],
  ArrowLeft: ['ArrowLeft', 37],
  ArrowUp: ['ArrowUp', 38],
  ArrowRight: ['ArrowRight', 39],
  ArrowDown: ['ArrowDown', 40],
  ShiftLeft: ['Shift', 16, null, 8, 1],
  ShiftRight: ['Shift', 16, null, 8, 2],
  ControlLeft: ['Control', 17, null, 2, 1],
  ControlRight: ['Control', 17, null, 2, 2],
  AltLeft: ['Alt', 18, null, 1, 1],
  AltRight: ['Alt', 18, null, 1, 2],
  MetaLeft: ['Meta', 91, null, 4, 1],
  MetaRight: ['Meta', 93, null, 4, 2],
};
const ALIASES = { Shift: 'ShiftLeft', Control: 'ControlLeft', Ctrl: 'ControlLeft', Alt: 'AltLeft', Option: 'AltLeft', Meta: 'MetaLeft', Cmd: 'MetaLeft', Command: 'MetaLeft' };

function keyFor(name) {
  const code = ALIASES[name] || name;
  if (NAMED[code]) {
    const [key, vk, text, modifier = 0, location = 0] = NAMED[code];
    return { code, key, vk, text, modifier, location };
  }
  let m = /^Key([A-Z])$/.exec(code);
  if (m) return { code, key: m[1].toLowerCase(), vk: m[1].charCodeAt(0), text: m[1].toLowerCase(), modifier: 0, location: 0 };
  m = /^Digit([0-9])$/.exec(code);
  if (m) return { code, key: m[1], vk: 48 + Number(m[1]), text: m[1], modifier: 0, location: 0 };
  bail(`unknown key code "${name}" (see --help for the names)`);
}

function parseSteps(spec) {
  return spec.split(',').map((s) => s.trim()).filter(Boolean).map((step) => {
    const [head, tail] = step.split(':');
    if (head === 'wait') return { kind: 'wait', ms: Number(tail), label: step };
    if (head === 'down' || head === 'up') return { kind: head, key: keyFor(tail), label: step };
    const ms = Number(tail);
    if (!Number.isFinite(ms)) bail(`bad step "${step}" (want Code:ms, down:Code, up:Code or wait:ms)`);
    return { kind: 'hold', key: keyFor(head), ms, label: step };
  });
}

// ---------------------------------------------------------------------------
// Chrome
// ---------------------------------------------------------------------------

function findChrome() {
  if (process.env.CHROME) return process.env.CHROME;
  const mac = [
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    '/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary',
  ];
  for (const c of mac) if (fs.existsSync(c)) return c;
  for (const c of ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser']) {
    try {
      return execFileSync('which', [c], { stdio: ['ignore', 'pipe', 'ignore'] }).toString().trim();
    } catch {}
  }
  return null;
}

async function launchChrome() {
  const bin = findChrome();
  if (!bin) bail('no Chrome found: set CHROME=/path/to/chrome, or pass --cdp <port>');
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'patina-playtest-chrome-'));
  const child = spawn(bin, [
    '--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`,
    '--no-first-run', '--no-default-browser-check',
    // A background tab must not be throttled: the frame pump is what we measure.
    '--disable-background-timer-throttling', '--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows', 'about:blank',
  ], { stdio: 'ignore' });
  const cleanup = () => {
    try { child.kill('SIGTERM'); } catch {}
    try { fs.rmSync(profile, { recursive: true, force: true }); } catch {}
  };
  // Chrome writes the port it picked to DevToolsActivePort.
  const portFile = path.join(profile, 'DevToolsActivePort');
  for (let i = 0; i < 200; i++) {
    if (fs.existsSync(portFile)) {
      const port = fs.readFileSync(portFile, 'utf8').split('\n')[0].trim();
      if (port) return { port, cleanup };
    }
    if (child.exitCode !== null) break;
    await sleep(100);
  }
  cleanup();
  bail(`Chrome (${bin}) did not open a DevTools port`);
}

async function openTab(port) {
  const target = await (await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: 'PUT' })).json();
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve, { once: true });
    ws.addEventListener('error', reject, { once: true });
  });
  let nextId = 1;
  const pending = new Map();
  const listeners = [];
  ws.addEventListener('message', (ev) => {
    const msg = JSON.parse(ev.data);
    if (msg.id && pending.has(msg.id)) {
      pending.get(msg.id)(msg);
      pending.delete(msg.id);
    } else if (msg.method) {
      for (const l of listeners) l(msg);
    }
  });
  // Every DevTools call gets a deadline: one call Chrome never answers (a
  // screenshot or an awaited evaluate under heavy load) otherwise left the
  // whole run waiting forever — a --profile run once sat silent for 6.5 hours
  // with the game still playing in the editor.
  const send = (method, params = {}, timeoutMs = 30000) => new Promise((resolve, reject) => {
    const id = nextId++;
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Chrome did not answer ${method} within ${timeoutMs / 1000}s`));
    }, timeoutMs);
    pending.set(id, (msg) => { clearTimeout(timer); resolve(msg); });
    ws.send(JSON.stringify({ id, method, params }));
  });
  const evaluate = async (expression) => {
    const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
    if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text);
    return r.result?.result?.value;
  };
  const close = async () => {
    try { ws.close(); } catch {}
    try { await fetch(`http://127.0.0.1:${port}/json/close/${target.id}`); } catch {}
  };
  return { send, evaluate, on: (fn) => listeners.push(fn), close };
}

// ---------------------------------------------------------------------------
// Profiling (macOS `sample`)
// ---------------------------------------------------------------------------

function editorPid(port) {
  try {
    return execFileSync('lsof', ['-nP', `-iTCP:${port}`, '-sTCP:LISTEN', '-t'], { stdio: ['ignore', 'pipe', 'ignore'] })
      .toString().trim().split('\n')[0] || null;
  } catch {
    return null;
  }
}

// How long the editor process has run and how much memory it holds. A
// long-lived editor misleads a measurement: one that had played for 6.5 hours
// rendered the platformer at 25 ms a frame where a fresh one took 15 ms.
function processInfo(pid) {
  if (!pid) return null;
  try {
    const [etime, rss] = execFileSync('ps', ['-o', 'etime=,rss=', '-p', String(pid)], { stdio: ['ignore', 'pipe', 'ignore'] })
      .toString().trim().split(/\s+/);
    // etime is [[dd-]hh:]mm:ss
    const [days, rest] = etime.includes('-') ? etime.split('-') : ['0', etime];
    const parts = rest.split(':').map(Number);
    while (parts.length < 3) parts.unshift(0);
    const age_s = Number(days) * 86400 + parts[0] * 3600 + parts[1] * 60 + parts[2];
    return { pid: Number(pid), age_s, rss_mb: Math.round(Number(rss) / 1024) };
  } catch {
    return null;
  }
}

// Waiting in the kernel is not work: these are the idle leaves of a sample.
const IDLE = /^(__semwait_signal|semaphore_|__psynch_|mach_msg|__workq_kernreturn|kevent|__select|__accept|__recvfrom|__recvmsg|__read_nocancel|read|__ulock_wait|_pthread_cond_wait|__sigsuspend|__wait4|poll|__poll)/;

function demangle(name) {
  return name
    .replace(/\s+\(in [^)]*\).*$/, '')
    .replace(/::h[0-9a-f]{16}$/, '')
    .replace(/\$LT\$/g, '<').replace(/\$GT\$/g, '>').replace(/\$u20\$/g, ' ')
    .replace(/\$u5b\$/g, '[').replace(/\$u5d\$/g, ']').replace(/\$u7b\$/g, '{').replace(/\$u7d\$/g, '}')
    .replace(/\$RF\$/g, '&').replace(/\$BP\$/g, '*').replace(/\$C\$/g, ',').replace(/\.\./g, '::')
    .replace(/^_</, '<');
}

function hottest(sampleText, limit = 12) {
  const at = sampleText.indexOf('Sort by top of stack');
  if (at < 0) return null;
  const rows = [];
  for (const line of sampleText.slice(at).split('\n').slice(1)) {
    const m = /^\s+(.+?)\s+(\d+)\s*$/.exec(line);
    if (!m) {
      if (rows.length) break;
      continue;
    }
    rows.push({ name: m[1], samples: Number(m[2]) });
  }
  const busy = rows.filter((r) => !IDLE.test(r.name));
  const total = rows.reduce((n, r) => n + r.samples, 0);
  const work = busy.reduce((n, r) => n + r.samples, 0);
  return {
    idle_share: total ? 1 - work / total : 0,
    top: busy.slice(0, limit).map((r) => ({ share: work ? r.samples / work : 0, name: demangle(r.name) })),
  };
}

// ---------------------------------------------------------------------------
// The playtest
// ---------------------------------------------------------------------------

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const steps = parseSteps(opts.keys);
  const editorUrl = new URL(opts.url);
  const api = `${editorUrl.protocol}//${editorUrl.host}`;
  const out = opts.out || fs.mkdtempSync(path.join(os.tmpdir(), 'patina-playtest-'));
  fs.mkdirSync(out, { recursive: true });
  // The editor's runtime status, or null if what answers is not one.
  const status = async () => {
    try {
      const res = await fetch(`${api}/api/runtime/status`);
      const body = res.ok ? await res.json() : null;
      return typeof body?.running === 'boolean' ? body : null;
    } catch {
      return null;
    }
  };
  const before = await status();
  if (!before) bail(`no Patina editor answers at ${api} (start one: scripts/editor-bg.sh start --release <project>)`);

  const chrome = opts.cdp ? { port: opts.cdp, cleanup: () => {} } : await launchChrome();
  process.on('SIGINT', () => { chrome.cleanup(); process.exit(2); });
  // A whole-run deadline on top of the per-call ones: the run's own waits
  // (key steps, profile, settle) plus generous slack. On expiry Play is
  // stopped so the editor is not left running a game nobody watches.
  const budgetMs = opts.budgetMs || 120000 + steps.reduce((a, s) => a + (s.ms || 0), 0) + (opts.soak || 0) * 1000 + (opts.profile || 0) * 1000 * 4;
  setTimeout(async () => {
    console.error(`browser-playtest: no result after ${Math.round(budgetMs / 1000)}s — giving up`);
    try { await fetch(`${api}/api/runtime/stop`, { method: 'POST' }); } catch {}
    chrome.cleanup();
    process.exit(2);
  }, budgetMs).unref();
  const tab = await openTab(chrome.port);
  const report = {
    url: opts.url, build: before.build || 'unknown', keys: opts.keys, problems: [], warnings: [],
    frames: [], http_errors: [], exceptions: [], console_errors: [], steps: [],
  };
  const pid = editorPid(editorUrl.port || 80);
  report.editor_process = processInfo(pid);
  if (report.editor_process?.age_s > 3600) {
    report.warnings.push(`the editor has been running ${Math.round(report.editor_process.age_s / 3600)} h — restart it before trusting a timing (scripts/editor-bg.sh start …)`);
  }
  if (report.editor_process?.rss_mb > 1536) {
    report.warnings.push(`the editor holds ${report.editor_process.rss_mb} MB — restart it before trusting a timing`);
  }
  // Other work on the host slows every number here; record it so a reader (and
  // the gate) can tell a regression from a busy machine.
  report.host = { load1: +os.loadavg()[0].toFixed(2), cpus: os.cpus().length };
  if (report.host.load1 > report.host.cpus) {
    report.warnings.push(`host load ${report.host.load1} exceeds ${report.host.cpus} CPUs — frame rates are not representative`);
  }
  const urls = new Map();
  const sizes = [];
  tab.on((msg) => {
    const p = msg.params;
    if (msg.method === 'Network.requestWillBeSent') {
      urls.set(p.requestId, { url: p.request.url, method: p.request.method });
    } else if (msg.method === 'Network.responseReceived') {
      const u = p.response.url.replace(api, '');
      const req = urls.get(p.requestId) || { method: 'GET' };
      if (/^\/api\/viewport(\?|$)/.test(u) && p.response.status === 200) {
        const h = p.response.headers;
        report.frames.push({ t: Date.now(), seq: h['X-Frame-Seq'] ?? h['x-frame-seq'] ?? null });
      }
      if (p.response.status >= 400 && !u.startsWith('/favicon')) {
        report.http_errors.push({ status: p.response.status, request: `${req.method} ${u.split('?')[0]}` });
      }
    } else if (msg.method === 'Network.loadingFinished') {
      const req = urls.get(p.requestId);
      if (req && /\/api\/viewport(\?|$)/.test(req.url.replace(api, ''))) sizes.push(p.encodedDataLength);
    } else if (msg.method === 'Runtime.exceptionThrown') {
      report.exceptions.push(p.exceptionDetails.exception?.description || p.exceptionDetails.text);
    } else if (msg.method === 'Runtime.consoleAPICalled' && p.type === 'error') {
      report.console_errors.push(p.args.map((a) => a.value ?? a.description).join(' '));
    }
  });

  try {
    await tab.send('Page.enable');
    await tab.send('Runtime.enable');
    await tab.send('Network.enable');
    await tab.send('Emulation.setDeviceMetricsOverride', { width: 1600, height: 1000, deviceScaleFactor: 1, mobile: false });
    await tab.send('Page.navigate', { url: opts.url });
    let ready = false;
    for (let i = 0; i < 100 && !ready; i++) {
      await sleep(150);
      ready = await tab.evaluate(`!!document.querySelector('#btn-play')`).catch(() => false);
    }
    if (!ready) throw new Error(`the editor page at ${opts.url} never showed a Play button`);
    await sleep(800);
    const shot = async (name) => {
      const r = await tab.send('Page.captureScreenshot', { format: 'png' });
      const file = path.join(out, `${name}.png`);
      fs.writeFileSync(file, Buffer.from(r.result.data, 'base64'));
      return file;
    };
    report.screenshots = [await shot('start')];

    // Edit-view actions, then: does every image the page references load?
    for (const raw of opts.editSteps.split(',').map((x) => x.trim()).filter(Boolean)) {
      const at = raw.indexOf(':');
      const [kind, arg] = at < 0 ? [raw, ''] : [raw.slice(0, at), raw.slice(at + 1)];
      if (kind === 'wait') await sleep(Number(arg));
      else if (kind === 'select') {
        // Expand collapsed rows (one depth per pass) until the node shows.
        let ok = false;
        for (let pass = 0; pass < 8 && !ok; pass++) {
          ok = await tab.evaluate(`(() => {
            const want = ${JSON.stringify(arg)};
            const hit = [...document.querySelectorAll('#scene-tree .tree-name')].find((e) => e.textContent.trim() === want);
            if (hit) { (hit.closest('.tree-row') || hit).click(); return true; }
            for (const t of document.querySelectorAll('#scene-tree .tree-toggle')) {
              if (t.textContent === '\u25B6') t.click();
            }
            return false;
          })()`);
          await sleep(300);
        }
        if (!ok) report.problems.push(`edit step ${raw}: no scene-tree row named "${arg}" (expand its parent?)`);
        await sleep(800);
      } else report.problems.push(`unknown edit step "${raw}" (want select:<name> or wait:<ms>)`);
    }
    report.images = await tab.evaluate(`(async () => {
      const seen = new Set();
      for (const el of document.querySelectorAll('img[src]')) seen.add(el.src);
      for (const el of document.querySelectorAll('[style*="url("]')) {
        const m = /url\\(["']?([^"')]+)["']?\\)/.exec(el.style.backgroundImage || '');
        if (m) seen.add(new URL(m[1], location.href).href);
      }
      const same = [...seen].filter((u) => u.startsWith(location.origin) && !u.includes('/api/viewport'));
      const out = [];
      for (const u of same) {
        try {
          const r = await fetch(u);
          out.push({ url: u.replace(location.origin, ''), status: r.status, type: r.headers.get('content-type') });
        } catch (e) {
          out.push({ url: u.replace(location.origin, ''), status: 0, type: String(e) });
        }
      }
      return out;
    })()`);
    const brokenImages = report.images.filter((i) => i.status !== 200 || !/^image\//.test(i.type || ''));
    if (brokenImages.length) {
      report.problems.push(`${brokenImages.length} of ${report.images.length} image(s) on the page do not load: ${brokenImages.slice(0, 3).map((i) => `${i.status} ${i.url}`).join(', ')}`);
    }

    // Editor-log entries from here on are this run's (a second of slack for
    // clock granularity); earlier ones belong to whoever played before.
    const playStart = Date.now() - 1000;
    await tab.evaluate(`document.querySelector('#btn-play').click()`);
    let running = false;
    for (let i = 0; i < 100 && !running; i++) {
      await sleep(100);
      running = (await status())?.running === true;
    }
    if (!running) {
      report.problems.push('Play did not start: is a project with a runnable scene open in the editor?');
      throw new Error('stop');
    }
    await sleep(1000); // the frame pump's first frames, and a full second of game rates

    let sampler = null;
    const sampleFile = path.join(out, 'sample.txt');
    if (opts.profile > 0) {
      if (process.platform !== 'darwin') report.warnings.push('--profile needs macOS `sample`; skipped');
      else if (!pid) report.warnings.push(`--profile: no process is listening on port ${editorUrl.port}`);
      else {
        // Capture the exit NOW: awaiting `on('exit')` after the key steps hung
        // forever whenever the profile was shorter than the steps, because
        // `sample` had already exited and the event fired with no listener.
        const proc = spawn('sample', [pid, String(opts.profile), '-file', sampleFile], { stdio: 'ignore' });
        sampler = new Promise((r) => { proc.on('exit', r); proc.on('error', r); });
      }
    }

    // The watched properties, read straight from the running game.
    // A node in the runtime scene tree by absolute path.
    const findInTree = (n, p) => {
      if (!n) return null;
      if (n.path === p) return n;
      for (const c of n.children || []) {
        const hit = findInTree(c, p);
        if (hit) return hit;
      }
      return null;
    };
    const readWatch = async (spec) => {
      const at = spec.lastIndexOf(':');
      const node = spec.slice(0, at), prop = spec.slice(at + 1);
      if (prop === '#children') {
        try {
          const d = await (await fetch(`${api}/api/runtime/scene`)).json();
          const hit = findInTree(d.tree, node);
          return hit ? String((hit.children || []).length) : `(no ${node} in the running scene)`;
        } catch (e) {
          return `(unreadable: ${e.message})`;
        }
      }
      try {
        const d = await (await fetch(`${api}/api/runtime/node?path=${encodeURIComponent(node)}`)).json();
        const found = (d.properties || []).find((p) => p.name === prop);
        return found ? JSON.stringify(found.value?.value ?? found.value) : `(no ${prop}: ${d.error?.message || 'not on the node'})`;
      } catch (e) {
        return `(unreadable: ${e.message})`;
      }
    };
    report.watch = opts.watch.map((spec) => ({ watch: spec, values: [] }));

    // Keys, with real auto-repeat and the modifier bits a browser would set.
    let modifiers = 0;
    const keyEvent = (type, k, autoRepeat = false) => tab.send('Input.dispatchKeyEvent', {
      type, key: k.key, code: k.code, windowsVirtualKeyCode: k.vk, nativeVirtualKeyCode: k.vk,
      text: type === 'keyDown' ? (k.text ?? undefined) : undefined,
      unmodifiedText: type === 'keyDown' ? (k.text ?? undefined) : undefined,
      autoRepeat, modifiers, location: k.location,
    });
    const scriptStart = Date.now();
    for (const step of steps) {
      const t0 = Date.now();
      if (step.kind === 'wait') await sleep(step.ms);
      else if (step.kind === 'down') {
        modifiers |= step.key.modifier;
        await keyEvent('keyDown', step.key);
      } else if (step.kind === 'up') {
        await keyEvent('keyUp', step.key);
        modifiers &= ~step.key.modifier;
      } else {
        await keyEvent('keyDown', step.key);
        await sleep(Math.min(step.ms, 400)); // the OS key-repeat delay
        while (Date.now() - t0 < step.ms) {
          await keyEvent('keyDown', step.key, true);
          await sleep(33); // ~30 Hz repeat, like macOS
        }
        await keyEvent('keyUp', step.key);
      }
      report.steps.push({ step: step.label, t0, t1: Date.now() });
      for (const w of report.watch) w.values.push({ after: step.label, value: await readWatch(w.watch) });
    }
    const scriptEnd = Date.now();

    // Soak: keep playing and watch for growth that a short run cannot show.
    if (opts.soak > 0) {
      report.soak = [];
      const t0 = Date.now();
      while (Date.now() - t0 < opts.soak * 1000) {
        const st = await status();
        const pi = processInfo(pid);
        report.soak.push({ t_s: Math.round((Date.now() - t0) / 1000), render_ms: st?.render_ms ?? null, sim_fps: st?.sim_fps ?? null, rss_mb: pi?.rss_mb ?? null });
        await sleep(10000);
      }
      // Ignore the first 30 s (texture cache, allocator warm-up).
      const warm = report.soak.filter((x) => x.t_s >= 30 && x.rss_mb != null);
      if (warm.length >= 3) {
        const a = warm[0], b = warm[warm.length - 1];
        const minutes = Math.max(1 / 60, (b.t_s - a.t_s) / 60);
        report.soak_rss_mb_per_min = +((b.rss_mb - a.rss_mb) / minutes).toFixed(1);
        const third = Math.max(1, Math.floor(warm.length / 3));
        const mean = (xs) => xs.reduce((p, x) => p + (x.render_ms || 0), 0) / xs.length;
        report.soak_render_ms = [+mean(warm.slice(0, third)).toFixed(1), +mean(warm.slice(-third)).toFixed(1)];
        if (report.soak_rss_mb_per_min > 10) {
          report.problems.push(`memory grew ${report.soak_rss_mb_per_min} MB/min over a ${opts.soak} s soak (${a.rss_mb} → ${b.rss_mb} MB)`);
        }
        const [early, late] = report.soak_render_ms;
        if (late > early * 1.3 && late - early > 3) {
          report.problems.push(`render time grew over the soak: ${early} → ${late} ms`);
        }
      }
    }
    for (const w of report.watch) {
      const readable = w.values.map((v) => v.value).filter((v) => !v.startsWith('('));
      if (readable.length >= 2 && new Set(readable).size === 1) {
        report.warnings.push(`${w.watch} stayed ${readable[0]} through every step: do the controls reach the game?`);
      } else if (!readable.length) {
        report.warnings.push(`${w.watch} was never readable: ${w.values[0]?.value ?? 'no steps ran'}`);
      }
    }
    if (sampler) await sampler;

    // What the player got.
    const unique = (frames) => new Set(frames.map((f, i) => f.seq ?? `#${i}`)).size;
    const inWindow = (a, b) => report.frames.filter((f) => f.t >= a && f.t <= b);
    const played = report.frames.filter((f) => f.t >= scriptStart && f.t <= scriptEnd);
    const seconds = Math.max(0.001, (scriptEnd - scriptStart) / 1000);
    report.shown_fps = +(unique(played) / seconds).toFixed(1);
    report.repeated_frames = played.length - unique(played);
    report.frame_kb = sizes.length ? Math.round(sizes.reduce((a, b) => a + b, 0) / sizes.length / 1024) : null;
    for (const s of report.steps) {
      const secs = Math.max(0.001, (s.t1 - s.t0) / 1000);
      s.shown_fps = +(unique(inWindow(s.t0, s.t1)) / secs).toFixed(1);
    }
    report.server = await status();
    report.page = await tab.evaluate(`(() => {
      const visible = (el) => !!el && el.offsetParent !== null && el.getBoundingClientRect().width > 0;
      const screen = document.getElementById('game-screen');
      const img = visible(screen) ? document.getElementById('game-screen-img')
        : document.querySelector('#viewport-container img');
      const overlay = document.getElementById('fps-overlay');
      const bar = document.getElementById('status-runtime');
      const page = {
        overlay_visible: visible(overlay),
        overlay: overlay ? overlay.innerText.replace(/\\n/g, ' | ') : null,
        status_bar: bar ? bar.innerText : null,
      };
      if (img && img.naturalWidth) {
        const rect = img.getBoundingClientRect();
        page.frame = [img.naturalWidth, img.naturalHeight];
        page.shown_at = [Math.round(rect.width), Math.round(rect.height)];
        // Pixels still at the runtime clear colour (0.3 grey = 76) are ones
        // the game never drew: the content box says how much of the view the
        // game fills.
        const c = document.createElement('canvas');
        c.width = img.naturalWidth; c.height = img.naturalHeight;
        const g = c.getContext('2d');
        g.drawImage(img, 0, 0);
        const d = g.getImageData(0, 0, c.width, c.height).data;
        let x0 = c.width, y0 = c.height, x1 = -1, y1 = -1;
        for (let y = 0; y < c.height; y += 2) for (let x = 0; x < c.width; x += 2) {
          const i = (y * c.width + x) * 4;
          if (Math.abs(d[i] - 76) > 3 || Math.abs(d[i + 1] - 76) > 3 || Math.abs(d[i + 2] - 76) > 3) {
            if (x < x0) x0 = x; if (x > x1) x1 = x; if (y < y0) y0 = y; if (y > y1) y1 = y;
          }
        }
        page.content = x1 < 0 ? null : [x0, y0, Math.min(c.width, x1 + 2) - x0, Math.min(c.height, y1 + 2) - y0];
      }
      return page;
    })()`);

    // The editor's own reading of the same moment.
    try {
      const r = await (await fetch(`${api}/mcp`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name: 'diagnose', arguments: {} } }),
      })).json();
      const text = (r.result?.content || []).filter((b) => b.type === 'text').map((b) => b.text).join('\n');
      report.diagnose = r.result?.isError ? null : text;
      if (!report.diagnose) report.warnings.push(`the editor's diagnose tool is unavailable: ${text || JSON.stringify(r.error || r)}`);
    } catch (e) {
      report.warnings.push(`the editor's diagnose tool is unavailable: ${e.message}`);
    }
    // What the speakers got. Play from the page opens the real device.
    if (opts.audio > 0) {
      try {
        const wav = path.join(out, 'audio.wav');
        const r = await (await fetch(`${api}/mcp`, {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ jsonrpc: '2.0', id: 2, method: 'tools/call', params: { name: 'audio', arguments: { secs: opts.audio, save: wav } } }),
        })).json();
        const text = (r.result?.content || []).filter((b) => b.type === 'text').map((b) => b.text).join('\n');
        report.audio = r.result?.isError ? null : text;
        if (!report.audio) report.warnings.push(`audio was not measured: ${text || JSON.stringify(r.error || r)}`);
        for (const line of (report.audio || '').split('\n')) {
          if (line.startsWith('finding: ')) report.problems.push(`audio: ${line.slice(9)}`);
        }
      } catch (e) {
        report.warnings.push(`audio was not measured: ${e.message}`);
      }
    }
    report.screenshots.push(await shot('end'));
    if (sampler && fs.existsSync(sampleFile)) {
      report.profile = hottest(fs.readFileSync(sampleFile, 'utf8'));
      report.profile_file = sampleFile;
    }

    // Verdict.
    if (!played.length) report.problems.push('the page received no frames while the game ran');
    else if (report.shown_fps < opts.minFps) {
      const msg = `shown ${report.shown_fps} frames/s, below --min-fps ${opts.minFps}`;
      // `sample` suspends the editor to walk its stacks every millisecond, so a
      // profiled run is slower than the game is: on the platformer demo the
      // same build showed 56 frames/s plain and 31 under --profile. Judge the
      // rate on a run without it.
      if (opts.profile > 0) report.warnings.push(`${msg} — measured under --profile, which slows the editor; re-run without it to judge the frame rate`);
      else report.problems.push(msg);
    }
    const refused = report.http_errors.filter((e) => e.status === 429);
    if (refused.length) report.problems.push(`${refused.length} request(s) refused with 429: ${[...new Set(refused.map((e) => e.request))].join(', ')}`);
    const broken = report.http_errors.filter((e) => e.status >= 500);
    if (broken.length) report.problems.push(`${broken.length} server error(s): ${[...new Set(broken.map((e) => `${e.status} ${e.request}`))].join(', ')}`);
    // A 4xx from the editor's own API is a broken feature, not noise: the tile
    // palette's thumbnails 404'd for weeks as a mere warning.
    const apiMissing = report.http_errors.filter((e) => e.status >= 400 && e.status < 500 && e.status !== 429 && / \/api\//.test(e.request));
    if (apiMissing.length) report.problems.push(`${apiMissing.length} editor API request(s) failed: ${[...new Set(apiMissing.map((e) => `${e.status} ${e.request}`))].slice(0, 4).join(', ')}`);
    const other = report.http_errors.filter((e) => e.status !== 429 && e.status < 500 && !apiMissing.includes(e));
    if (other.length) report.warnings.push(`${other.length} 4xx response(s): ${[...new Set(other.map((e) => `${e.status} ${e.request}`))].join(', ')}`);
    if (report.exceptions.length) report.problems.push(`${report.exceptions.length} uncaught page exception(s): ${report.exceptions[0]}`);
    if (report.console_errors.length) report.problems.push(`${report.console_errors.length} console error(s): ${report.console_errors[0]}`);

    // The GAME's own errors — script parse failures, `method not found` every
    // frame — land in the editor log, not the page console. Without this, a
    // game whose player script never even parsed reported "problems: none".
    try {
      const logs = await (await fetch(`${api}/api/logs`)).json();
      const mine = (Array.isArray(logs) ? logs : logs.entries || [])
        .filter((e) => e.level === 'error' && (e.timestamp ?? 0) >= playStart);
      const counts = new Map();
      for (const e of mine) counts.set(e.message, (counts.get(e.message) || 0) + 1);
      report.script_errors = [...counts].map(([message, count]) => ({ message, count }));
      for (const { message, count } of report.script_errors.slice(0, 8)) {
        report.problems.push(`game error${count > 1 ? ` ×${count}` : ''}: ${message}`);
      }
      if (report.script_errors.length > 8) report.problems.push(`…and ${report.script_errors.length - 8} more distinct game errors (report.json)`);
    } catch (e) {
      report.warnings.push(`could not read the editor log: ${e.message}`);
    }
    if (report.page && !report.page.overlay_visible) report.warnings.push('the FPS counter is not visible over the game');
    if (report.page?.frame && report.page.content) {
      const [w, h] = report.page.frame;
      const share = (report.page.content[2] * report.page.content[3]) / (w * h);
      report.page.content_share = +share.toFixed(2);
      if (share < 0.6) {
        report.warnings.push(`the game fills ${Math.round(share * 100)}% of the ${w}x${h} view (${report.page.content[2]}x${report.page.content[3]} at ${report.page.content[0]},${report.page.content[1]}): is the project's stretch mode applied?`);
      }
    }
    for (const line of (report.diagnose || '').split('\n')) {
      if (line.startsWith('- ')) report.warnings.push(`editor: ${line.slice(2)}`);
    }
  } catch (e) {
    if (e.message !== 'stop') report.problems.push(`playtest failed: ${e.message}`);
  } finally {
    if (!opts.keepRunning) {
      await tab.evaluate(`(() => { const b = document.querySelector('#btn-stop'); if (b) b.click(); })()`).catch(() => {});
      await sleep(300);
    }
    await tab.close();
    chrome.cleanup();
  }

  delete report.frames;
  fs.writeFileSync(path.join(out, 'report.json'), JSON.stringify(report, null, 2));
  if (opts.json) console.log(JSON.stringify(report, null, 2));
  else print(report, out);
  process.exit(report.problems.length ? 1 : 0);
}

function print(r, out) {
  const lines = [`Patina browser playtest: ${r.url} (${r.build} build)`];
  lines.push(`  keys: ${r.keys}`);
  if (r.shown_fps !== undefined) {
    lines.push(`  shown: ${r.shown_fps} frames/s${r.repeated_frames ? `, ${r.repeated_frames} repeated` : ''}${r.frame_kb ? `, ${r.frame_kb} KB per frame` : ''}`);
    // A rate over a step shorter than half a second is mostly noise.
    const long = r.steps.filter((s) => s.t1 - s.t0 >= 500);
    if (long.length > 1) lines.push(`  per step: ${long.map((s) => `${s.step} ${s.shown_fps}/s`).join(' · ')}`);
  }
  // Coordinates rounded to one decimal: the trend is the point, not the noise.
  const tidy = (v) => v.replace(/-?\d+\.\d+/g, (n) => String(Math.round(Number(n) * 10) / 10));
  for (const w of r.watch || []) {
    lines.push(`  watch ${w.watch}: ${w.values.map((v) => `${v.after} → ${tidy(v.value)}`).join(' · ')}`);
  }
  if (r.server) lines.push(`  server: game ${r.server.sim_fps ?? '?'}/s · render ${r.server.render_fps ?? '?'}/s at ${r.server.render_ms ?? '?'} ms`);
  if (r.page) {
    lines.push(`  counter: ${r.page.overlay ? `"${r.page.overlay}"` : '(none)'}${r.page.overlay_visible ? '' : ' (not visible)'}`);
    if (r.page.frame) {
      const content = r.page.content ? `game content ${r.page.content[2]}x${r.page.content[3]} at ${r.page.content[0]},${r.page.content[1]} (${Math.round((r.page.content_share ?? 0) * 100)}%)` : 'no game content drawn';
      lines.push(`  view: frame ${r.page.frame.join('x')} shown at ${r.page.shown_at.join('x')}; ${content}`);
    }
  }
  if (r.diagnose) lines.push('  editor diagnose:', ...r.diagnose.trimEnd().split('\n').map((l) => `    ${l}`));
  if (r.audio) lines.push('  audio device:', ...r.audio.trimEnd().split('\n').map((l) => `    ${l}`));
  if (r.profile) {
    lines.push(`  hottest functions (${Math.round(r.profile.idle_share * 100)}% of samples idle; ${r.profile_file}):`);
    for (const f of r.profile.top) lines.push(`    ${String(Math.round(f.share * 100)).padStart(3)}%  ${f.name}`);
  }
  lines.push(r.problems.length ? `problems (${r.problems.length}):` : 'problems: none');
  for (const p of r.problems) lines.push(`  - ${p}`);
  if (r.warnings.length) lines.push(`warnings (${r.warnings.length}):`, ...r.warnings.map((w) => `  - ${w}`));
  lines.push(`report + screenshots: ${out}`);
  console.log(lines.join('\n'));
}

main().catch((e) => bail(e.stack || String(e)));
