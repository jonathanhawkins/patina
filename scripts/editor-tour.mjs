#!/usr/bin/env node
// Tour the Patina browser EDITOR itself — not Play — the way a person
// exploring it would, and report what is broken or poor in each area.
//
//   scripts/editor-tour.mjs <game_dir> [options]
//
//   --port <P>        editor port (default $PATINA_PORT or 8080)
//   --scene <path>    scene to open, relative to the game (default: the
//                     project's run/main_scene)
//   --out <dir>       report.md, report.json, contact-sheet.png, one
//                     screenshot per area and the editor's log, editor.log
//                     (default: a temp dir)
//   --size <WxH>      browser window size (default 1600x1000); a small one
//                     (1280x720) shows what a laptop user gets
//   --json            print the report as JSON instead of the summary
//   --no-start        tour the editor already running on the port (it must
//                     have <game_dir> open); default: (re)start a RELEASE
//                     editor on the port with scripts/editor-bg.sh, and stop
//                     it again afterwards
//   --build           with the default start: build the editor first
//   --keep-running    leave the editor running afterwards
//   --only <regex>    tour only the areas whose name matches
//   --cdp <port>      drive an already running Chrome's DevTools port
//
// Areas: the scene dock; the inspector after selecting a 2D, a 3D, a Control,
// a scripted node and an instanced scene (whichever the main scene has); the
// FileSystem dock; the 2D, 3D, Script and AssetLib main screens; every bottom
// panel tab (Output, Debugger, Audio, Animation, Shader Editor, ...); every
// main menu; the Add Node, Project Settings and Editor Settings dialogs.
//
// Each area records a screenshot, console errors and warnings, uncaught page
// exceptions, failed (4xx/5xx) requests, broken images, layout problems
// (boxes running off the window, zero-size panels, overlapping controls,
// text clipped with no ellipsis), panels that are empty when they must show
// something (the inspector with no properties for a selected node), and the
// time until the area shows its content. Analysis: scripts/lib/editor-tour-lib.mjs.
//
// Exit status: 0 no hard findings · 1 hard findings · 2 could not run.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { launchChrome, openTab, sleep } from './lib/cdp.mjs';
import {
  collectArea, layoutFindings, classify, flattenScene, pickNodes, mainSceneFromProject, renderMarkdown, contactSheetHtml, exitCode, foldRepeats,
} from './lib/editor-tour-lib.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function usage() {
  const lines = fs.readFileSync(new URL(import.meta.url), 'utf8').split('\n').slice(1);
  const end = lines.findIndex((l) => !l.startsWith('//'));
  return lines.slice(0, end).map((l) => l.replace(/^\/\/ ?/, '')).join('\n');
}

function bail(message) {
  console.error(`editor-tour: ${message}`);
  process.exit(2);
}

function parseArgs(argv) {
  const opts = { game: null, scene: null, width: 1600, height: 1000, port: process.env.PATINA_PORT || '8080', out: null, json: false, start: true, build: false, keepRunning: false, only: null, cdp: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => {
      if (i + 1 >= argv.length) bail(`${a} needs a value`);
      return argv[++i];
    };
    if (a === '--port') opts.port = value();
    else if (a === '--out') opts.out = value();
    else if (a === '--scene') opts.scene = value();
    else if (a === '--size') {
      const m = /^(\d+)x(\d+)$/.exec(value());
      if (!m) bail('--size wants WxH, e.g. 1280x720');
      opts.width = Number(m[1]);
      opts.height = Number(m[2]);
    }
    else if (a === '--json') opts.json = true;
    else if (a === '--no-start') opts.start = false;
    else if (a === '--build') opts.build = true;
    else if (a === '--keep-running') opts.keepRunning = true;
    else if (a === '--only') opts.only = new RegExp(value(), 'i');
    else if (a === '--cdp') opts.cdp = value();
    else if (a === '-h' || a === '--help') { console.log(usage()); process.exit(0); }
    else if (a.startsWith('-')) bail(`unknown option ${a} (see --help)`);
    else if (!opts.game) opts.game = a;
    else bail(`unexpected argument ${a}`);
  }
  if (!opts.game) bail('need a <game_dir> (see --help)');
  if (!fs.existsSync(path.join(opts.game, 'project.godot'))) bail(`${opts.game} has no project.godot`);
  return opts;
}

// ---------------------------------------------------------------------------
// Areas
// ---------------------------------------------------------------------------

// Page-side helpers every area step can call (installed once per page load).
const PAGE_HELPERS = `
window.__tour = {
  visible(sel) {
    const el = typeof sel === 'string' ? document.querySelector(sel) : sel;
    if (!el) return false;
    const r = el.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) return false;
    for (let e = el; e; e = e.parentElement) {
      const cs = getComputedStyle(e);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
    }
    return true;
  },
  click(sel) {
    const el = typeof sel === 'string' ? document.querySelector(sel) : sel;
    if (!el) return false;
    el.scrollIntoView({ block: 'nearest' });
    const r = el.getBoundingClientRect();
    const o = { bubbles: true, cancelable: true, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2, button: 0 };
    el.dispatchEvent(new MouseEvent('mousedown', o));
    el.dispatchEvent(new MouseEvent('mouseup', o));
    el.click();
    return true;
  },
  count(sel) { return [...document.querySelectorAll(sel)].filter((e) => this.visible(e)).length; },
  // Expand collapsed scene-tree rows (one depth per call) until the node shows.
  selectNode(id) {
    const row = document.querySelector('#scene-tree .tree-row[data-node-id="' + id + '"]');
    if (row) { row.scrollIntoView({ block: 'center' }); (row.querySelector('.tree-name') || row).click(); return 'clicked'; }
    let opened = 0;
    for (const t of document.querySelectorAll('#scene-tree .tree-toggle')) if (t.textContent === '\\u25B6') { t.click(); opened++; }
    return opened ? 'expanding' : 'absent';
  },
  // Put the page back to rest: close menus, dialogs and popups.
  rest() {
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true }));
    document.querySelectorAll('.menu-item.open, .menu-item.active').forEach((m) => m.classList.remove('open', 'active'));
    for (const id of ['add-node-dialog', 'project-settings-dialog', 'settings-dialog', 'export-dialog', 'help-dialog', 'confirm-dialog', 'open-scene-dialog', 'save-scene-dialog', 'open-project-dialog', 'command-palette', 'quick-open-dialog']) {
      const d = document.getElementById(id);
      if (!d) continue;
      d.classList.remove('open');
      if (d.style.display && d.style.display !== 'none') d.style.display = 'none';
    }
  },
};
true`;

const MAIN_SCREEN_PANELS = ['#left-panel', '#center-area', '#inspector-panel'];

function buildAreas(picks) {
  const areas = [];
  areas.push({
    name: 'Scene dock',
    root: '#scene-panel',
    panels: ['#scene-panel', '#scene-tree'],
    expect: [{ sel: '#scene-tree .tree-row', min: 1, what: 'scene-tree rows' }],
    ready: `__tour.count('#scene-tree .tree-row') > 0`,
  });
  for (const p of picks) {
    areas.push({
      name: `Inspector: ${p.role} ${p.name} (${p.class})`,
      select: p.id,
      root: '#inspector-panel',
      panels: ['#inspector-panel', '#inspector-content'],
      expect: [
        { sel: '#inspector-content .insp-object-name', equals: p.name, min: 1, what: `the inspector header naming ${p.name}` },
        { sel: '#inspector-content .insp-row', min: 3, what: `property rows for ${p.class}` },
      ],
      // Ready once the inspector names THIS node: the previous node's rows
      // are still on screen until the new ones arrive.
      ready: `[...document.querySelectorAll('#inspector-content .insp-object-name')].some((e) => e.textContent.trim() === ${JSON.stringify(p.name)}) && __tour.count('#inspector-content .insp-row') > 0`,
      budget_ms: 1500,
    });
  }
  areas.push({
    name: 'FileSystem dock',
    root: '#filesystem-panel',
    panels: ['#filesystem-panel'],
    steps: [`__tour.click('#fs-dock-tab')`],
    expect: [{ sel: '#filesystem-panel #fs-tree *[data-path], #filesystem-panel #fs-file-list *[data-path], #filesystem-panel .fs-item, #filesystem-panel .fs-row', min: 2, what: 'files and folders' }],
    ready: `__tour.count('#filesystem-panel [data-path], #filesystem-panel .fs-item, #filesystem-panel .fs-row') > 1`,
  });
  const modes = [
    { mode: '2d', name: '2D main screen', panels: ['#viewport-panel', '#viewport-container'] },
    { mode: '3d', name: '3D main screen', panels: ['#viewport-panel', '#viewport-container'] },
    { mode: 'script', name: 'Script main screen', panels: ['#script-main-host'] },
    { mode: 'assetlib', name: 'AssetLib (asset browser)', panels: ['#assetlib-panel'], expect: [{ sel: '#assetlib-grid > *', min: 1, what: 'asset cards (or an explanation why there are none)' }] },
  ];
  for (const m of modes) {
    areas.push({
      name: m.name,
      root: 'body',
      panels: [...MAIN_SCREEN_PANELS.filter((x) => x !== '#center-area' || m.mode !== 'assetlib'), ...m.panels],
      steps: [`__tour.click('.mode-btn[data-mode="${m.mode}"]')`],
      expect: m.expect || [],
      ready: `document.querySelector('.mode-btn[data-mode="${m.mode}"]')?.classList.contains('active')`,
      after: `__tour.click('.mode-btn[data-mode="2d"]')`,
    });
  }
  areas.push({ bottomTabs: true });
  for (const menu of ['scene', 'edit', 'project', 'debug', 'editor', 'help']) {
    areas.push({
      name: `Menu: ${menu}`,
      root: `.menu-item[data-menu="${menu}"] .menu-dropdown`,
      panels: [`.menu-item[data-menu="${menu}"] .menu-dropdown`],
      expect: [{ sel: `.menu-item[data-menu="${menu}"] .menu-action`, min: 1, what: 'menu entries' }],
      steps: [`__tour.click('.menu-item[data-menu="${menu}"] > button')`],
      ready: `__tour.visible('.menu-item[data-menu="${menu}"] .menu-dropdown')`,
      after: `__tour.rest()`,
    });
  }
  areas.push({
    name: 'Dialog: Add Node',
    root: '#add-node-dialog',
    panels: ['#add-node-dialog-inner', '#add-node-list', '#add-node-description'],
    expect: [{ sel: '#add-node-list *', min: 5, what: 'node classes' }],
    steps: [`__tour.click('#btn-add')`],
    ready: `__tour.visible('#add-node-dialog') && __tour.count('#add-node-list *') > 0`,
    after: `__tour.rest()`,
  });
  areas.push({
    name: 'Dialog: Project Settings',
    root: '#project-settings-dialog',
    panels: ['#project-settings-dialog', '#pset-categories', '#pset-props'],
    expect: [{ sel: '#pset-props input, #pset-props select, #pset-props textarea', min: 1, what: 'editable settings' }],
    steps: [`__tour.click('.menu-item[data-menu="project"] > button')`, `__tour.click('.menu-action[data-action="project-settings"]')`],
    ready: `__tour.visible('#project-settings-dialog')`,
    after: `__tour.rest()`,
  });
  areas.push({
    name: 'Dialog: Editor Settings',
    root: '#settings-dialog',
    panels: ['#settings-dialog', '#settings-dialog-inner'],
    expect: [{ sel: '#settings-dialog input, #settings-dialog select', min: 1, what: 'editable settings' }],
    steps: [`__tour.click('.menu-item[data-menu="editor"] > button')`, `__tour.click('.menu-action[data-action="editor-settings"]')`],
    ready: `__tour.visible('#settings-dialog')`,
    after: `__tour.rest()`,
  });
  return areas;
}

// ---------------------------------------------------------------------------
// The tour
// ---------------------------------------------------------------------------

// The project's main scene as a path relative to the game, resolving a
// uid:// reference through the scenes' own `uid=` headers.
function mainScene(game) {
  const want = mainSceneFromProject(fs.readFileSync(path.join(game, 'project.godot'), 'utf8'));
  if (!want) return null;
  if (want.startsWith('res://')) return want.slice('res://'.length);
  const stack = [game];
  while (stack.length) {
    const dir = stack.pop();
    for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
      if (ent.name.startsWith('.')) continue;
      const p = path.join(dir, ent.name);
      if (ent.isDirectory()) stack.push(p);
      else if (ent.name.endsWith('.tscn')) {
        const head = fs.readFileSync(p, 'utf8').split('\n', 1)[0];
        if (head.includes(`uid="${want}"`)) return path.relative(game, p);
      }
    }
  }
  return null;
}

function editorBg(args) {
  return execFileSync(path.join(ROOT, 'scripts', 'editor-bg.sh'), args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const api = `http://127.0.0.1:${opts.port}`;
  const url = `http://localhost:${opts.port}/editor`;
  const out = path.resolve(opts.out || fs.mkdtempSync(path.join(os.tmpdir(), 'patina-tour-')));
  fs.mkdirSync(out, { recursive: true });
  const game = path.resolve(opts.game);
  const report = { game: path.basename(game), size: `${opts.width}x${opts.height}`, url, when: new Date().toISOString(), out, areas: [] };

  let started = false;
  if (opts.start) {
    const scene = opts.scene || mainScene(game);
    if (!scene) bail(`${game}/project.godot names no run/main_scene I can find; pass --scene`);
    report.scene = scene;
    try {
      editorBg(['start', '--release', ...(opts.build ? ['--build'] : []), '--port', String(opts.port), game, scene]);
      started = true;
    } catch (e) {
      bail(`could not start the editor on port ${opts.port}: ${(e.stderr || e.message || '').toString().trim().split('\n').slice(-3).join(' | ')}`);
    }
  }
  try {
    const st = await (await fetch(`${api}/api/runtime/status`)).json();
    report.build = st.build;
  } catch {
    bail(`no Patina editor answers at ${api}`);
  }

  const chrome = opts.cdp ? { port: opts.cdp, cleanup: () => {} } : await launchChrome().catch((e) => bail(e.message));
  const stopAll = () => {
    chrome.cleanup();
    // Keep the editor's own output with the report: the next start on this
    // port overwrites it, and it holds the panic if the editor died.
    try { fs.copyFileSync(path.join(process.env.TMPDIR || '/tmp', `patina-editor-${opts.port}.log`), path.join(out, 'editor.log')); } catch {}
    if (started && !opts.keepRunning) { try { editorBg(['stop', '--port', String(opts.port)]); } catch {} }
  };
  process.on('SIGINT', () => { stopAll(); process.exit(2); });
  setTimeout(() => {
    console.error('editor-tour: no result after 6 min — giving up');
    stopAll();
    process.exit(2);
  }, 360000).unref();

  const tab = await openTab(chrome.port);
  // Events go to whichever bucket is current: the page load, then each area.
  let bucket = { exceptions: [], console_errors: [], console_warnings: [], http_errors: [] };
  const inflight = new Map();
  let lastNet = Date.now();
  tab.on((msg) => {
    const p = msg.params;
    if (msg.method === 'Network.requestWillBeSent') {
      inflight.set(p.requestId, { url: p.request.url, method: p.request.method, long: /\/api\/(viewport|stream|events)/.test(p.request.url) });
      lastNet = Date.now();
    } else if (msg.method === 'Network.responseReceived') {
      const req = inflight.get(p.requestId) || { method: 'GET' };
      const u = p.response.url.replace(/^https?:\/\/[^/]+/, '');
      if (p.response.status >= 400) bucket.http_errors.push({ status: p.response.status, request: `${req.method} ${u.split('?')[0]}` });
    } else if (msg.method === 'Network.loadingFinished' || msg.method === 'Network.loadingFailed') {
      const req = inflight.get(p.requestId);
      if (msg.method === 'Network.loadingFailed' && req && !p.canceled && !req.long) {
        bucket.http_errors.push({ status: 0, request: `${req.method} ${req.url.replace(/^https?:\/\/[^/]+/, '').split('?')[0]} (${p.errorText})` });
      }
      inflight.delete(p.requestId);
      lastNet = Date.now();
    } else if (msg.method === 'Runtime.exceptionThrown') {
      bucket.exceptions.push((p.exceptionDetails.exception?.description || p.exceptionDetails.text || '').split('\n').slice(0, 2).join(' '));
    } else if (msg.method === 'Runtime.consoleAPICalled' && (p.type === 'error' || p.type === 'warning')) {
      const text = p.args.map((a) => a.value ?? a.description).join(' ').slice(0, 300);
      (p.type === 'error' ? bucket.console_errors : bucket.console_warnings).push(text);
    } else if (msg.method === 'Log.entryAdded' && (p.entry.level === 'error' || p.entry.level === 'warning')) {
      // Network failures are already counted from the Network domain.
      if (p.entry.source === 'network') return;
      (p.entry.level === 'error' ? bucket.console_errors : bucket.console_warnings).push(`[${p.entry.source}] ${p.entry.text}`.slice(0, 300));
    }
  });
  const quiet = async (ms = 300, max = 4000) => {
    const t0 = Date.now();
    while (Date.now() - t0 < max) {
      const busy = [...inflight.values()].some((r) => !r.long);
      if (!busy && Date.now() - lastNet >= ms) return;
      await sleep(50);
    }
  };
  const waitFor = async (expr, max = 10000) => {
    const t0 = Date.now();
    while (Date.now() - t0 < max) {
      if (await tab.evaluate(`(() => { try { return !!(${expr}); } catch (e) { return false; } })()`).catch(() => false)) return Date.now() - t0;
      await sleep(50);
    }
    return null;
  };
  const shot = async (name) => {
    const r = await tab.send('Page.captureScreenshot', { format: 'png' });
    const file = path.join(out, `${String(report.areas.length).padStart(2, '0')}-${name.replace(/[^a-z0-9]+/gi, '-').replace(/^-|-$/g, '').toLowerCase()}.png`);
    fs.writeFileSync(file, Buffer.from(r.result.data, 'base64'));
    return file;
  };
  const checkedImages = new Set();
  const brokenImages = async () => {
    const found = await tab.evaluate(`(() => {
      const out = [];
      for (const el of document.querySelectorAll('img')) {
        if (!__tour.visible(el) || !el.currentSrc && !el.src) continue;
        const broken = el.complete && el.naturalWidth === 0;
        out.push({ url: el.currentSrc || el.src, broken });
      }
      return out;
    })()`);
    const res = [];
    for (const f of found) {
      if (f.url.startsWith('data:') && !f.broken) continue;
      if (f.url.includes("/api/viewport")) continue;
      const key = `${f.url}|${f.broken}`;
      if (checkedImages.has(key)) continue;
      checkedImages.add(key);
      if (f.broken) res.push({ url: f.url.replace(/^https?:\/\/[^/]+/, '').slice(0, 160), why: 'the browser could not decode it (naturalWidth 0)' });
    }
    return res;
  };

  const alive = async () => {
    try { return (await fetch(`${api}/api/runtime/status`, { signal: AbortSignal.timeout(3000) })).ok; } catch { return false; }
  };
  const logTail = () => {
    const log = path.join(process.env.TMPDIR || '/tmp', `patina-editor-${opts.port}.log`);
    try {
      const lines = fs.readFileSync(log, 'utf8').trimEnd().split('\n');
      const at = lines.findIndex((l) => /panicked at/.test(l));
      const tail = at >= 0 ? lines.slice(at, at + 4) : lines.slice(-4);
      return ` — ${log}: ${tail.join(' | ').slice(0, 600)}`;
    } catch {
      return '';
    }
  };
  const runArea = async (spec) => {
    const a = { name: spec.name };
    bucket = { exceptions: [], console_errors: [], console_warnings: [], http_errors: [] };
    const t0 = Date.now();
    try {
      if (spec.select != null) {
        let state = 'absent';
        for (let pass = 0; pass < 10; pass++) {
          state = await tab.evaluate(`__tour.selectNode(${JSON.stringify(spec.select)})`);
          if (state === 'clicked') break;
          await sleep(250);
        }
        if (state !== 'clicked') throw new Error(`no scene-tree row for node id ${spec.select}`);
      }
      const steps = spec.steps || [];
      for (let i = 0; i < steps.length; i++) {
        await tab.evaluate(steps[i]);
        if (i < steps.length - 1) await sleep(150); // let a menu open before its entry is clicked
      }
      const ready = await waitFor(spec.ready || 'true', 8000);
      a.ready_ms = ready == null ? null : Date.now() - t0;
      if (ready == null) a.error = `never became ready (${spec.ready})`;
      await quiet();
      await sleep(250);
      a.budget_ms = spec.budget_ms ?? 3000;
      a.screenshot = await shot(spec.name);
      const data = await tab.evaluate(`(${collectArea.toString()})(${JSON.stringify({ root: spec.root, panels: spec.panels, expect: spec.expect })})`);
      a.layout = layoutFindings(data);
      a.scanned = data.elements.length;
      a.broken_images = await brokenImages();
    } catch (e) {
      a.error = e.message;
      try { a.screenshot = a.screenshot || await shot(spec.name); } catch {}
    }
    try { if (spec.after) await tab.evaluate(spec.after); else await tab.evaluate('__tour.rest()'); } catch {}
    await sleep(200);
    Object.assign(a, bucket);
    // A refused connection means the editor process is gone: say so once,
    // with the end of its log (the panic), instead of 30 areas of noise.
    if (a.http_errors.some((r) => /ERR_CONNECTION_REFUSED/.test(r.request)) && !(await alive())) {
      a.error = `the editor on port ${opts.port} stopped answering during this area${logTail()}`;
      a.http_errors = a.http_errors.filter((r) => !/ERR_CONNECTION_REFUSED/.test(r.request));
      Object.assign(a, classify(a));
      report.areas.push(a);
      throw Object.assign(new Error(a.error), { died: true });
    }
    Object.assign(a, classify(a));
    report.areas.push(a);
    if (!opts.json) console.error(`  ${a.hard.length ? 'FAIL' : 'ok  '} ${a.name}${a.ready_ms != null ? ` (${a.ready_ms} ms)` : ''}${a.hard.length ? ` — ${a.hard.length} hard` : ''}${a.soft.length ? `, ${a.soft.length} soft` : ''}`);
  };

  try {
    await tab.send('Page.enable');
    await tab.send('Runtime.enable');
    await tab.send('Network.enable');
    await tab.send('Log.enable');
    await tab.send('Emulation.setDeviceMetricsOverride', { width: opts.width, height: opts.height, deviceScaleFactor: 1, mobile: false });
    const t0 = Date.now();
    await tab.send('Page.navigate', { url });
    let ok = null;
    for (let i = 0; i < 200 && !ok; i++) {
      await sleep(100);
      ok = await tab.evaluate(`!!document.querySelector('#btn-play') && document.querySelectorAll('#scene-tree .tree-row').length > 0`).catch(() => false);
    }
    report.load_ms = ok ? Date.now() - t0 : null;
    await tab.evaluate(PAGE_HELPERS);
    await quiet(500, 8000);
    // The page load is an area of its own: whatever broke while it loaded.
    const load = { name: 'Page load', ready_ms: report.load_ms, budget_ms: 5000, screenshot: await shot('page-load') };
    if (!ok) load.error = 'the editor never showed a Play button and a scene tree';
    load.layout = layoutFindings(await tab.evaluate(`(${collectArea.toString()})(${JSON.stringify({ root: 'body', panels: MAIN_SCREEN_PANELS })})`));
    load.broken_images = await brokenImages();
    Object.assign(load, bucket, classify({ ...load, ...bucket }));
    report.areas.push(load);
    if (!opts.json) console.error(`  ${load.hard.length ? 'FAIL' : 'ok  '} Page load (${report.load_ms} ms)`);
    if (!ok) throw Object.assign(new Error('editor page did not load'), { fatal: true });

    let picks = [];
    try {
      const scene = await (await fetch(`${api}/api/scene`)).json();
      picks = pickNodes(flattenScene(scene.nodes));
    } catch (e) {
      report.areas.push({ name: 'Scene API', error: e.message, ...classify({ error: `/api/scene: ${e.message}` }) });
    }
    report.picks = picks.map((p) => ({ role: p.role, name: p.name, class: p.class, path: p.path }));
    for (const spec of buildAreas(picks)) {
      if (spec.bottomTabs) {
        const tabs = await tab.evaluate(`[...document.querySelectorAll('#bottom-panel-header .bottom-tab')].filter((b) => __tour.visible(b)).map((b) => ({ tab: b.dataset.tab, label: b.textContent.trim() }))`);
        for (const t of tabs) {
          const s = {
            name: `Bottom panel: ${t.label}`,
            root: '#bottom-panel',
            panels: ['#bottom-panel', `.bottom-content-tab[data-tab="${t.tab}"]`],
            steps: [`(() => { const p = document.getElementById('bottom-panel'); const b = document.querySelector('.bottom-tab[data-tab="${t.tab}"]'); if (!b.classList.contains('active') || p.classList.contains('collapsed')) b.click(); if (p.classList.contains('collapsed')) b.click(); })()`],
            ready: `!document.getElementById('bottom-panel').classList.contains('collapsed') && __tour.visible('.bottom-content-tab[data-tab="${t.tab}"]')`,
            after: `(() => { const p = document.getElementById('bottom-panel'); if (!p.classList.contains('collapsed')) document.querySelector('.bottom-tab[data-tab="${t.tab}"]').click(); })()`,
          };
          if (!opts.only || opts.only.test(s.name)) await runArea(s);
        }
        continue;
      }
      if (opts.only && !opts.only.test(spec.name)) continue;
      await runArea(spec);
    }

  } catch (e) {
    if (e.died) report.editor_died = e.message;
    else if (e.fatal || !report.areas.length) report.fatal = e.message;
    else report.areas.push({ name: 'Tour', error: e.message, ...classify({ error: e.message }) });
  }
  // Whatever happened, fold the repeats and photograph what was seen.
  try {
    foldRepeats(report.areas);
    // Contact sheet: lay the screenshots out in a page and photograph it.
    const shots = report.areas.filter((a) => a.screenshot && fs.existsSync(a.screenshot)).map((a) => ({
      name: a.name, hard: a.hard.length, dataUrl: `data:image/png;base64,${fs.readFileSync(a.screenshot).toString('base64')}`,
    }));
    if (shots.length) {
      const html = path.join(out, 'contact-sheet.html');
      fs.writeFileSync(html, contactSheetHtml(`Editor tour: ${report.game} — ${report.when}`, shots));
      await tab.send('Emulation.setDeviceMetricsOverride', { width: 4 * 410 + 34, height: 800, deviceScaleFactor: 1, mobile: false });
      await tab.send('Page.navigate', { url: `file://${html}` });
      await sleep(1500);
      const r = await tab.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true }, 60000);
      report.contact_sheet = path.join(out, 'contact-sheet.png');
      fs.writeFileSync(report.contact_sheet, Buffer.from(r.result.data, 'base64'));
      fs.rmSync(html, { force: true });
    }
  } catch (e) {
    console.error(`editor-tour: no contact sheet: ${e.message}`);
  } finally {
    await tab.close();
    stopAll();
  }

  fs.writeFileSync(path.join(out, 'report.json'), JSON.stringify(report, null, 2));
  fs.writeFileSync(path.join(out, 'report.md'), renderMarkdown(report));
  if (opts.json) console.log(JSON.stringify(report, null, 2));
  else {
    const hard = report.areas.flatMap((a) => a.hard.map((h) => `  [${a.name}] ${h}`));
    console.log(`editor-tour ${report.game}: ${hard.length ? 'FAIL' : 'OK'} — ${report.areas.length} areas, ${hard.length} hard finding(s)${report.fatal ? ` — could not run: ${report.fatal}` : ''}`);
    for (const h of hard.slice(0, 40)) console.log(h);
    if (hard.length > 40) console.log(`  ... ${hard.length - 40} more`);
    console.log(`report: ${path.join(out, 'report.md')}`);
    if (report.contact_sheet) console.log(`contact sheet: ${report.contact_sheet}`);
  }
  process.exit(exitCode(report));
}

main().catch((e) => bail(e.stack || e.message));
