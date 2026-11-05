// Layout regressions in the editor page itself, found by scripts/editor-tour.mjs
// and checked here without an editor: the page is the EDITOR_HTML string in
// engine-rs/crates/gdeditor/src/editor_ui.rs, laid out in a headless Chrome
// with its scripts off (so no server is needed), and judged by the tour's own
// layout checks. Skipped when no Chrome is installed.

import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { findChrome, launchChrome, openTab, sleep } from '../lib/cdp.mjs';
import { collectArea, overlappingControls, clippedText } from '../lib/editor-tour-lib.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const SRC = path.join(ROOT, 'engine-rs', 'crates', 'gdeditor', 'src', 'editor_ui.rs');

/// The page exactly as the editor serves it (serve_editor_html sends the
/// constant verbatim).
export function editorHtml() {
  const src = fs.readFileSync(SRC, 'utf8');
  const start = src.indexOf('pub const EDITOR_HTML: &str = r##"');
  assert.ok(start >= 0, 'EDITOR_HTML not found in editor_ui.rs');
  const body = src.slice(start + 'pub const EDITOR_HTML: &str = r##"'.length);
  return body.slice(0, body.indexOf('"##;'));
}

const skip = findChrome() ? false : 'no Chrome installed';
let chrome;
let tab;
let file;
const logs = [];

before(async () => {
  if (skip) return;
  file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'patina-layout-')), 'editor.html');
  fs.writeFileSync(file, editorHtml());
  chrome = await launchChrome();
  tab = await openTab(chrome.port);
  await tab.send('Page.enable');
  await tab.send('Log.enable');
  tab.on((msg) => { if (msg.method === 'Log.entryAdded') logs.push(msg.params.entry); });
  await tab.send('Emulation.setScriptExecutionDisabled', { value: true });
});

after(async () => {
  if (tab) await tab.close();
  if (chrome) chrome.cleanup();
  if (file) fs.rmSync(path.dirname(file), { recursive: true, force: true });
});

// A laptop-sized window, where both bugs showed on platformer_3d.
async function layoutAt(width, height, title) {
  await tab.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
  await tab.send('Page.navigate', { url: `file://${file}` });
  for (let i = 0; i < 100; i++) {
    await sleep(50);
    if (await tab.evaluate(`document.readyState === 'complete'`)) break;
  }
  // _update_title()'s text for platformer_3d: "<scene file> - <project name>".
  await tab.evaluate(`document.getElementById('project-title').textContent = ${JSON.stringify(title)}`);
  // The bottom panel tabs are what a user sees with the panel collapsed.
  return tab.evaluate(`(${collectArea.toString()})({ root: 'body' })`);
}

test('the project title never runs under the main-screen buttons (1280x720)', { skip }, async () => {
  const data = await layoutAt(1280, 720, 'game.tscn - Platformer 3D');
  const hits = overlappingControls(data).filter((f) => f.sel.includes('project-title'));
  assert.deepEqual(hits, [], 'the title overlapped a mode button: it must shrink with an ellipsis instead');
  // ...and it says it was shortened rather than cutting the text silently.
  assert.deepEqual(clippedText(data).filter((f) => f.sel.includes('project-title')), []);
});

test('the title still shows in full when there is room (1920x1080)', { skip }, async () => {
  await layoutAt(1920, 1080, 'game.tscn - Platformer 3D');
  // The text's own width (a Range ignores clipping) fits in the box drawn.
  const full = await tab.evaluate(`(() => {
    const e = document.getElementById('project-title');
    const r = document.createRange(); r.selectNodeContents(e);
    return e.getBoundingClientRect().width + 1 >= r.getBoundingClientRect().width && r.getBoundingClientRect().width > 50;
  })()`);
  assert.equal(full, true);
});

test('no bottom-panel tab label is cut off by the 30px tab strip (1280x720)', { skip }, async () => {
  const data = await layoutAt(1280, 720, 'x');
  const cut = clippedText(data).filter((f) => /bottom-tab|tab-btn/.test(f.sel));
  assert.deepEqual(cut, [], 'a tab label wrapped onto more lines than the strip is tall');
  // Each tab is one line tall.
  const tall = await tab.evaluate(`[...document.querySelectorAll('#bottom-panel-header .bottom-tab')].filter((b) => b.offsetParent && b.getBoundingClientRect().height > 28).map((b) => b.textContent.trim())`);
  assert.deepEqual(tall, []);
});

test('with too many tabs for the strip, every tab and the version label stay visible, none overlapping (1280x720)', { skip }, async () => {
  await layoutAt(1280, 720, 'x');
  // Shown the way an open panel shows it (Pin/Expand only show while open).
  const onScreen = await tab.evaluate(`(() => {
    document.getElementById('bottom-panel').classList.remove('collapsed');
    const strip = document.getElementById('bottom-panel-header').getBoundingClientRect();
    const b = document.getElementById('bottom-version').getBoundingClientRect();
    const inside = (r) => r.width > 8 && r.left >= strip.left - 1 && r.right <= strip.right + 1;
    const tabs = [...document.querySelectorAll('#bottom-panel-header .bottom-tab')].filter((t) => t.offsetParent);
    return inside(b) && tabs.length > 5 && tabs.every((t) => inside(t.getBoundingClientRect()));
  })()`);
  assert.equal(onScreen, true);
  const data = await tab.evaluate(`(${collectArea.toString()})({ root: '#bottom-panel-header' })`);
  assert.deepEqual(overlappingControls(data), []);
});

test('an audio bus fader is vertical without the deprecated slider-vertical appearance', { skip }, async () => {
  logs.length = 0;
  await layoutAt(1280, 720, 'x');
  // The Audio panel builds its faders from JS; make one the same way.
  const shape = await tab.evaluate(`(() => {
    const host = document.querySelector('.bottom-content-tab[data-tab="audio"]');
    document.getElementById('bottom-panel').classList.remove('collapsed');
    host.classList.add('active');
    const f = document.createElement('input');
    f.type = 'range'; f.className = 'abus-fader';
    host.appendChild(f);
    const r = f.getBoundingClientRect();
    return { w: r.width, h: r.height };
  })()`);
  // Chrome reports the deprecation once the fader is painted.
  await tab.send('Page.captureScreenshot', { format: 'png' });
  await sleep(500);
  assert.ok(shape.h > shape.w * 3, `the fader is ${shape.w}x${shape.h}: not vertical`);
  assert.deepEqual(logs.filter((e) => /slider-vertical/.test(e.text)).map((e) => e.text), []);
});

test('at 1600x1000 with a bottom panel open, every tab label shows in full', { skip }, async () => {
  await layoutAt(1600, 1000, 'x');
  const cut = await tab.evaluate(`(() => {
    // The dock widths the editor's JS gives a 1600px window (measured on the
    // tour's dodge_the_creeps screenshot): Scene dock 280, Inspector 320.
    document.getElementById('left-panel').style.width = '280px';
    document.getElementById('inspector-panel').style.width = '320px';
    document.getElementById('bottom-panel').classList.remove('collapsed');
    return [...document.querySelectorAll('#bottom-panel-header .bottom-tab')]
      .filter((t) => t.offsetParent && t.scrollWidth > t.clientWidth + 1).map((t) => t.textContent.trim());
  })()`);
  assert.deepEqual(cut, [], 'tabs were shortened with an ellipsis although the window is wide');
});

test('a Color property\'s alpha field shows "1.00" in full', { skip }, async () => {
  await layoutAt(1600, 1000, 'x');
  // Built the way the inspector builds a Color row (insp-row > insp-value >
  // color-editor > color-slider-group > number input), in the inspector.
  const data = await tab.evaluate(`(() => {
    const host = document.getElementById('inspector-content');
    host.innerHTML = '<div class="insp-row"><span class="insp-label">Modulate</span><div class="insp-value"><div class="color-editor">'
      + '<div class="color-swatch"></div><input type="text" class="color-hex" value="#ffffff">'
      + '<div class="color-slider-group"><span class="color-slider-label">A</span><input type="number" min="0" max="1" step="0.05" value="1.00"></div>'
      + '</div></div></div>';
    return (${collectArea.toString()})({ root: '#inspector-content' });
  })()`);
  assert.ok(data.elements.some((e) => e.tag === 'input' && e.text === '1.00'), 'the alpha field was laid out');
  assert.deepEqual(clippedText(data).filter((f) => f.detail.includes('1.00')), []);
});
