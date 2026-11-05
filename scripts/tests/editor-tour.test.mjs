// Tests for the editor tour's analysis: `node --test scripts/tests/*.test.mjs`.
//
// The tour itself needs a live editor and Chrome; everything that decides a
// verdict (layout checks on DOM rects, failure classification, which nodes
// get inspected, the report) is pure and tested here on synthetic data.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { findChrome } from '../lib/cdp.mjs';
import {
  viewportOverflow, zeroSizePanels, overlappingControls, clippedText, emptyPanels, dedupe, layoutFindings,
  classify, nodeKind, flattenScene, pickNodes, mainSceneFromProject, renderMarkdown, contactSheetHtml, exitCode, collectArea, foldRepeats,
} from '../lib/editor-tour-lib.mjs';

// An element as collectArea describes it. `vis` defaults to `rect`.
let next = 0;
function el(props) {
  const rect = props.rect || [0, 0, 10, 10];
  return { i: next++, parent: -1, sel: `div#e${next}`, tag: 'div', rect, vis: rect, control: false, text: '', ...props };
}
function page(elements, extra = {}) {
  elements.forEach((e, i) => { e.i = i; });
  return { vw: 1600, vh: 1000, elements, panels: [], expect: [], ...extra };
}

// --- viewport overflow ------------------------------------------------------

test('a box running off the right edge is reported with how far', () => {
  const d = page([el({ sel: 'div#toolbar', rect: [1200, 0, 500, 30] })]);
  const f = viewportOverflow(d);
  assert.equal(f.length, 1);
  assert.equal(f[0].sel, 'div#toolbar');
  assert.match(f[0].detail, /100px past the right edge/);
});

test('a box inside the window, or over by under the slack, is fine', () => {
  const d = page([el({ rect: [0, 0, 1600, 1000] }), el({ rect: [1590, 990, 11, 11] })]);
  assert.deepEqual(viewportOverflow(d), []);
});

test('only the outermost offender of a nest is reported', () => {
  const outer = el({ sel: 'div#bar', rect: [1500, 0, 300, 20] });
  const inner = el({ sel: 'button#b', rect: [1700, 0, 50, 20], parent: 0 });
  const d = page([outer, inner]);
  const f = viewportOverflow(d);
  assert.deepEqual(f.map((x) => x.sel), ['div#bar']);
});

test('a box an ancestor clips away (vis null) cannot overflow', () => {
  const d = page([el({ rect: [1700, 0, 50, 20], vis: null })]);
  assert.deepEqual(viewportOverflow(d), []);
});

test('overflow past the bottom and the left edge are both named', () => {
  const f = viewportOverflow(page([el({ rect: [-40, 900, 100, 200] })]));
  assert.match(f[0].detail, /100px past the bottom/);
  assert.match(f[0].detail, /40px past the left edge/);
});

// --- zero-size / missing panels --------------------------------------------

test('a displayed panel with zero height is zero-size; a sized one is not', () => {
  const d = page([], { panels: [
    { sel: '#inspector', present: true, displayed: true, w: 300, h: 0 },
    { sel: '#scene', present: true, displayed: true, w: 300, h: 400 },
    { sel: '#gone', present: false },
    { sel: '#hid', present: true, displayed: false, w: 0, h: 0 },
  ] });
  const f = zeroSizePanels(d);
  assert.deepEqual(f.map((x) => [x.kind, x.sel]), [['zero-size', '#inspector'], ['missing-panel', '#gone'], ['hidden-panel', '#hid']]);
  assert.equal(f[0].detail, '300x0');
});

// --- overlapping controls ---------------------------------------------------

test('two uncovered buttons sharing most of their box overlap', () => {
  const a = el({ sel: 'button#a', control: true, top: true, rect: [100, 100, 40, 20] });
  const b = el({ sel: 'button#b', control: true, top: true, rect: [110, 100, 40, 20] });
  const f = overlappingControls(page([a, b]));
  assert.equal(f.length, 1);
  assert.equal(f[0].sel, 'button#a / button#b');
  assert.match(f[0].detail, /30x20/);
});

test('buttons that merely touch, or overlap by a sliver, do not', () => {
  const a = el({ control: true, top: true, rect: [100, 100, 40, 20] });
  const b = el({ control: true, top: true, rect: [140, 100, 40, 20] });
  const c = el({ control: true, top: true, rect: [178, 100, 40, 20] }); // 2px of 40
  assert.deepEqual(overlappingControls(page([a, b, c])), []);
});

test('a control under a dialog (not on top) is not an overlap', () => {
  const a = el({ control: true, top: false, rect: [100, 100, 40, 20] });
  const b = el({ control: true, top: true, rect: [100, 100, 40, 20] });
  assert.deepEqual(overlappingControls(page([a, b])), []);
});

test('a control nested in another control is not an overlap', () => {
  const a = el({ control: true, top: true, rect: [100, 100, 80, 20] });
  const b = el({ control: true, top: true, rect: [110, 100, 20, 20], parent: 0 });
  assert.deepEqual(overlappingControls(page([a, b])), []);
});

test('non-controls stacked on each other are not an overlap', () => {
  const a = el({ rect: [0, 0, 100, 100] });
  const b = el({ rect: [0, 0, 100, 100] });
  assert.deepEqual(overlappingControls(page([a, b])), []);
});

// --- clipped text -----------------------------------------------------------

test('text cut by its own box with no ellipsis is clipped; with one it is not', () => {
  const a = el({ sel: 'span.name', text: 'CollisionShape2D', clipX: true, textOverflow: 'clip' });
  const b = el({ sel: 'span.ok', text: 'CollisionShape2D', clipX: true, textOverflow: 'ellipsis' });
  const c = el({ sel: 'span.fits', text: 'Node', clipX: false, textOverflow: 'clip' });
  const d = el({ sel: 'span.hidden', text: 'Node', clipX: true, textOverflow: 'clip', vis: null });
  const f = clippedText(page([a, b, c, d]));
  assert.deepEqual(f.map((x) => x.sel), ['span.name']);
  assert.match(f[0].detail, /CollisionShape2D/);
});

// --- empty panels -----------------------------------------------------------

test('an inspector showing no property rows for a selected node is empty', () => {
  const d = page([], { expect: [
    { sel: '#inspector-content .insp-row', what: 'property rows', min: 3, count: 0 },
    { sel: '#scene-tree .tree-row', what: 'rows', min: 1, count: 4 },
  ] });
  const f = emptyPanels(d);
  assert.equal(f.length, 1);
  assert.equal(f[0].kind, 'empty');
  assert.match(f[0].detail, /property rows: 0 shown, want at least 3/);
});

test('exactly the minimum is not empty', () => {
  assert.deepEqual(emptyPanels(page([], { expect: [{ sel: 'x', min: 3, count: 3 }] })), []);
});

// --- dedupe and the combined check ------------------------------------------

test('identical findings collapse into one with a count', () => {
  const f = dedupe([
    { kind: 'clipped-text', sel: 'span.tree-name', detail: 'a' },
    { kind: 'clipped-text', sel: 'span.tree-name', detail: 'b' },
    { kind: 'overflow', sel: 'span.tree-name', detail: 'c' },
  ]);
  assert.equal(f.length, 2);
  assert.equal(f[0].count, 2);
  assert.equal(f[1].count, 1);
});

test('a missing area root is reported, with its missing panels and empty content', () => {
  assert.deepEqual(layoutFindings({ missing: true }).map((f) => f.kind), ['missing-root']);
  const f = layoutFindings({ missing: true, panels: [{ sel: '#p', present: false }], expect: [{ sel: '.r', min: 1, count: 0 }], elements: [] });
  assert.deepEqual(f.map((x) => x.kind), ['missing-root', 'missing-panel', 'empty']);
});

test('layoutFindings runs every check', () => {
  const d = page([
    el({ sel: 'div#off', rect: [1550, 0, 200, 20] }),
    el({ sel: 'span.cut', text: 'long label', clipX: true, textOverflow: 'clip', rect: [0, 0, 10, 10] }),
  ], {
    panels: [{ sel: '#p', present: true, displayed: true, w: 0, h: 10 }],
    expect: [{ sel: '.row', what: 'rows', min: 1, count: 0 }],
  });
  assert.deepEqual(layoutFindings(d).map((f) => f.kind).sort(), ['clipped-text', 'empty', 'overflow', 'zero-size']);
});

// --- classification ---------------------------------------------------------

test('exceptions, console errors, 4xx/5xx, broken images and empty panels are hard', () => {
  const { hard, soft } = classify({
    exceptions: ['TypeError: x is undefined'],
    console_errors: ['boom'],
    console_warnings: ['deprecated'],
    http_errors: [{ status: 500, request: 'GET /api/x' }, { status: 404, request: 'GET /api/icon/Foo' }, { status: 404, request: 'GET /favicon.ico' }],
    broken_images: [{ url: '/api/thumb?p=a.png', why: 'not decodable' }],
    layout: [
      { kind: 'empty', sel: '#inspector-content .insp-row', detail: 'rows: 0', count: 1 },
      { kind: 'zero-size', sel: '#p', detail: '0x10', count: 1 },
      { kind: 'overflow', sel: '#bar', detail: '10px past the right edge', count: 1 },
      { kind: 'overlap', sel: 'a / b', detail: '10x10', count: 1 },
      { kind: 'clipped-text', sel: 'span', detail: 'cut', count: 7 },
    ],
  });
  assert.equal(hard.length, 7);
  assert.ok(hard.some((h) => h.startsWith('page exception: TypeError')));
  assert.ok(hard.some((h) => h === 'HTTP 404: GET /api/icon/Foo'));
  assert.ok(!hard.some((h) => h.includes('favicon')), 'a favicon probe is benign');
  assert.ok(hard.some((h) => h.startsWith('empty: #inspector-content')));
  assert.deepEqual(soft.map((s) => s.split(':')[0]), ['console warning', 'overflow', 'overlap', 'clipped-text']);
  assert.match(soft[3], /\(x7\)$/);
});

test('an area that never opened is hard; a slow one is soft', () => {
  assert.deepEqual(classify({ error: 'never became ready' }).hard, ['could not open: never became ready']);
  const slow = classify({ ready_ms: 4000, budget_ms: 3000 });
  assert.deepEqual(slow.hard, []);
  assert.match(slow.soft[0], /4000 ms/);
  assert.deepEqual(classify({ ready_ms: 100, budget_ms: 3000 }).soft, []);
});

test('exit code: 0 clean, 1 hard findings, 2 fatal', () => {
  assert.equal(exitCode({ areas: [{ hard: [], soft: ['x'] }] }), 0);
  assert.equal(exitCode({ areas: [{ hard: [], soft: [] }, { hard: ['y'], soft: [] }] }), 1);
  assert.equal(exitCode({ fatal: 'no editor', areas: [] }), 2);
  assert.equal(exitCode({ editor_died: 'panicked at x', areas: [{ hard: [], soft: [] }] }), 1, 'an editor that died is a failure');
  assert.match(renderMarkdown({ game: 'g', url: 'u', when: 'w', editor_died: 'panicked at foo.rs:1', areas: [] }), /\*\*the editor died\*\*.*panicked at foo\.rs:1/);
});

// --- picking nodes ----------------------------------------------------------

test('node kinds by class name', () => {
  assert.equal(nodeKind('Sprite2D'), '2d');
  assert.equal(nodeKind('CanvasLayer'), '2d');
  assert.equal(nodeKind('MeshInstance3D'), '3d');
  assert.equal(nodeKind('Label'), 'control');
  assert.equal(nodeKind('VBoxContainer'), 'control');
  assert.equal(nodeKind('Timer'), 'other');
});

test('pickNodes takes one of each kind, distinct nodes, never the root', () => {
  const tree = { id: 1, name: 'root', class: 'Node2D', has_script: true, children: [
    { id: 2, name: 'Main', class: 'Node', has_script: true, children: [
      { id: 3, name: 'Player', class: 'Area2D', has_script: true, is_instance: true, children: [] },
      { id: 4, name: 'HUD', class: 'CanvasLayer', has_script: true, is_instance: true, children: [
        { id: 5, name: 'Score', class: 'Label', children: [] },
      ] },
      { id: 6, name: 'Timer', class: 'Timer', children: [] },
    ] },
  ] };
  const picks = pickNodes(flattenScene(tree));
  assert.deepEqual(picks.map((p) => [p.role, p.name]), [
    ['instanced scene', 'Player'], ['scripted', 'Main'], ['2D', 'HUD'], ['Control', 'Score'],
  ]);
  assert.ok(!picks.some((p) => p.id === 1));
});

test('pickNodes reuses a node when no other one fits', () => {
  const tree = { id: 1, name: 'root', class: 'Node', children: [{ id: 2, name: 'Only', class: 'Sprite2D', has_script: true, children: [] }] };
  assert.deepEqual(pickNodes(flattenScene(tree)).map((p) => [p.role, p.name]), [['scripted', 'Only'], ['2D', 'Only']]);
});

// --- report -----------------------------------------------------------------

test('the markdown report has a verdict, a table row per area, and the findings', () => {
  const md = renderMarkdown({
    game: 'pong', url: 'http://localhost:1/editor', build: 'release', when: '2026-11-05', load_ms: 900,
    contact_sheet: '/tmp/x/contact-sheet.png',
    areas: [
      { name: 'Scene dock', ready_ms: 40, hard: [], soft: [], screenshot: '/tmp/x/01.png' },
      { name: 'Inspector: 2D | Paddle', ready_ms: 120, hard: ['empty: no rows'], soft: ['overflow: x'], screenshot: null },
    ],
  });
  assert.match(md, /^# Editor tour: pong/);
  assert.match(md, /verdict: \*\*FAIL\*\* — 1 hard, 1 soft finding\(s\) across 2 areas/);
  assert.match(md, /\| Scene dock \| 40 \| 0 \| 0 \| \/tmp\/x\/01.png \|/);
  assert.match(md, /Inspector: 2D \\\| Paddle/, 'a pipe in a name is escaped in the table');
  assert.match(md, /## Inspector: 2D \| Paddle\n- \*\*HARD\*\* empty: no rows\n- soft: overflow: x/);
  assert.ok(!md.includes('## Scene dock'), 'a clean area gets no section');
});

test('a clean report says OK', () => {
  const md = renderMarkdown({ game: 'g', url: 'u', when: 'w', areas: [{ name: 'a', hard: [], soft: [] }] });
  assert.match(md, /verdict: \*\*OK\*\* — 0 hard, 0 soft/);
});

test('the contact sheet marks areas with hard findings and escapes names', () => {
  const html = contactSheetHtml('T', [{ name: 'a<b', dataUrl: 'data:,', hard: 2 }, { name: 'ok', dataUrl: 'data:,', hard: 0 }]);
  assert.match(html, /<figure class="bad">/);
  assert.match(html, /a&lt;b — 2 hard/);
  assert.equal((html.match(/<figure/g) || []).length, 2);
});

test('collectArea is self-contained (shipped into the page with toString)', () => {
  // Evaluate its source in a scope with nothing from this module: a reference
  // to a module-level name would throw ReferenceError once the page runs it.
  const src = collectArea.toString();
  const fn = new Function(`return (${src})`)();
  const fakeDoc = { querySelector: () => null, querySelectorAll: () => [] };
  const g = globalThis;
  const saved = { document: g.document, window: g.window };
  g.document = fakeDoc;
  g.window = { innerWidth: 800, innerHeight: 600 };
  try {
    const out = fn({ root: '#nope', panels: ['#p'], expect: [{ sel: '.x', min: 2 }] });
    assert.equal(out.missing, true);
    assert.deepEqual(out.panels, [{ sel: '#p', present: false }]);
    assert.deepEqual(out.expect[0].count, 0);
  } finally {
    g.document = saved.document;
    g.window = saved.window;
  }
});

test('mainSceneFromProject reads run/main_scene from [application] only', () => {
  const text = '[gd_resource]\nrun/main_scene="res://wrong.tscn"\n\n[application]\n\nconfig/name="Pong"\nrun/main_scene="res://pong.tscn"\n\n[display]\n';
  assert.equal(mainSceneFromProject(text), 'res://pong.tscn');
  assert.equal(mainSceneFromProject('[application]\nrun/main_scene="uid://abc"\n'), 'uid://abc');
  assert.equal(mainSceneFromProject('[application]\nconfig/name="x"\n'), null);
});

test('foldRepeats lists a page-wide finding once, in the first area, with a count', () => {
  const areas = foldRepeats([
    { name: 'a', hard: ['H'], soft: ['overlap: title', 'only-a'] },
    { name: 'b', hard: ['H'], soft: ['overlap: title'] },
    { name: 'c', hard: [], soft: ['overlap: title', 'only-c'] },
  ]);
  assert.deepEqual(areas[0].hard, ['H [also in 1 later area]']);
  assert.deepEqual(areas[0].soft, ['overlap: title [also in 2 later areas]', 'only-a']);
  assert.deepEqual(areas[1], { name: 'b', hard: [], soft: [] });
  assert.deepEqual(areas[2].soft, ['only-c']);
  assert.equal(exitCode({ areas }), 1, 'folding keeps the hard verdict');
});

test('a label running under a button overlaps; two labels never do', () => {
  const title = el({ sel: 'span#project-title', text: 'game.tscn - Platformer 3D', top: true, rect: [420, 8, 160, 16] });
  const btn = el({ sel: 'button.mode-btn', control: true, top: true, text: '2D', rect: [560, 4, 40, 24] });
  const other = el({ sel: 'span.other', text: 'x', top: true, rect: [420, 8, 160, 16] });
  const f = overlappingControls(page([title, btn, other]));
  assert.deepEqual(f.map((x) => x.sel), ['span#project-title / button.mode-btn', 'button.mode-btn / span.other']);
});

test('text cut by a non-scrolling ancestor, or wrapped past its own box, is clipped', () => {
  const tab = el({ sel: 'button.bottom-tab', text: 'Find in Files', cut: [0, 17] });
  const wrapped = el({ sel: 'div.cell', text: 'Rotation Edit Mode', clipY: true });
  const tiny = el({ sel: 'span.ok', text: 'ok', cut: [1, 2] });
  const f = clippedText(page([tab, wrapped, tiny]));
  assert.deepEqual(f.map((x) => x.sel), ['button.bottom-tab', 'div.cell']);
  assert.match(f[0].detail, /loses 17px of its height to an ancestor that does not scroll/);
  assert.match(f[1].detail, /wraps taller than its box/);
});

// --- end to end, against a fake editor --------------------------------------

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

/// Just enough editor for the page to "load": a Play button and a scene-tree
/// row. It also logs a console error and references an image that 404s, and
/// it has no #scene-panel, so the Scene dock area must fail.
function fakeEditor() {
  const server = http.createServer((req, res) => {
    const json = (o) => { res.setHeader('Content-Type', 'application/json'); res.end(JSON.stringify(o)); };
    if (req.url.startsWith('/api/runtime/status')) return json({ running: false, build: 'test' });
    if (req.url.startsWith('/api/scene')) return json({ nodes: { id: 1, name: 'root', class: 'Node', children: [] } });
    if (req.url.startsWith('/editor')) {
      res.setHeader('Content-Type', 'text/html');
      return res.end(`<!doctype html><body><button id="btn-play">Play</button>
        <div id="scene-tree"><div class="tree-row">root</div></div>
        <img src="/icons/missing.png" width="16" height="16">
        <script>console.error('boom from the page')</script></body>`);
    }
    res.statusCode = 404; res.end('nope');
  });
  return new Promise((r) => server.listen(0, '127.0.0.1', () => r({ server, port: server.address().port })));
}

test('the tour reports a console error, a 404, a broken image and a missing panel, and exits 1', { skip: findChrome() ? false : 'no Chrome' }, async () => {
  const ed = await fakeEditor();
  const out = fs.mkdtempSync(path.join(os.tmpdir(), 'tour-test-'));
  try {
    const r = await new Promise((resolve) => {
      const p = spawn(process.execPath, [path.join(ROOT, 'scripts', 'editor-tour.mjs'), path.join(ROOT, 'games', 'pong'),
        '--no-start', '--port', String(ed.port), '--only', '^Scene dock$', '--out', out], { stdio: ['ignore', 'pipe', 'pipe'] });
      let so = '';
      p.stdout.on('data', (d) => (so += d));
      p.stderr.on('data', (d) => (so += d));
      const killer = setTimeout(() => p.kill('SIGKILL'), 90000);
      p.on('exit', (code) => { clearTimeout(killer); resolve({ code, out: so }); });
    });
    assert.equal(r.code, 1, r.out);
    const report = JSON.parse(fs.readFileSync(path.join(out, 'report.json'), 'utf8'));
    const load = report.areas.find((a) => a.name === 'Page load');
    assert.ok(load.hard.some((h) => h.includes('console error: boom from the page')), load.hard.join('\n'));
    assert.ok(load.hard.some((h) => h.includes('HTTP 404: GET /icons/missing.png')), load.hard.join('\n'));
    assert.ok(load.hard.some((h) => h.startsWith('broken image: /icons/missing.png')), load.hard.join('\n'));
    const dock = report.areas.find((a) => a.name === 'Scene dock');
    assert.ok(dock.hard.some((h) => h.startsWith('missing-panel: #scene-panel')), dock.hard.join('\n'));
    assert.deepEqual(report.areas.map((a) => a.name), ['Page load', 'Scene dock'], '--only limits the tour');
    assert.ok(fs.existsSync(path.join(out, 'report.md')));
    assert.ok(fs.existsSync(path.join(out, 'contact-sheet.png')));
  } finally {
    ed.server.close();
    fs.rmSync(out, { recursive: true, force: true });
  }
});
