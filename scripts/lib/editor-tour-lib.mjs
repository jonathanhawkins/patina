// Pure analysis behind scripts/editor-tour.mjs, kept apart so `node --test`
// can drive it with synthetic DOM rects (the tour itself needs a live editor
// and a Chrome).
//
// The page side (`collectArea`) is shipped into the editor tab with
// `fn.toString()`, so it must stay SELF-CONTAINED: it may not reference
// anything outside its own body.

// ---------------------------------------------------------------------------
// Page side
// ---------------------------------------------------------------------------

/// Runs IN THE PAGE. Describes every rendered element under `spec.root`
/// (default: the whole page) for the layout checks below, plus the size of
/// each `spec.panels` selector and the count behind each `spec.expect` entry.
///
/// Per element: `rect` is its border box; `vis` is that box clipped by every
/// ancestor that clips (overflow other than visible) — but NOT by the
/// viewport, so a box that runs off the window still shows it — or null when
/// an ancestor hides it entirely. `parent` is the index of the nearest
/// collected ancestor. `top` (controls only) says whether a click at the
/// centre of `vis` lands on the control itself, i.e. nothing covers it.
export function collectArea(spec) {
  const root = spec.root ? document.querySelector(spec.root) : document.body;
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const out = { vw, vh, missing: !root, elements: [], panels: [], expect: [] };
  const describe = (el) => {
    let s = el.tagName.toLowerCase();
    if (el.id) return `${s}#${el.id}`;
    const cls = typeof el.className === 'string' ? el.className.trim().split(/\s+/).filter(Boolean).slice(0, 2) : [];
    if (cls.length) s += '.' + cls.join('.');
    const p = el.parentElement;
    if (p && p.id) s = `#${p.id} > ${s}`;
    else if (p && typeof p.className === 'string' && p.className.trim()) s = `.${p.className.trim().split(/\s+/)[0]} > ${s}`;
    return s;
  };
  const shown = (el) => {
    if (!el.isConnected) return false;
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0 && el.getClientRects().length === 0) return false;
    for (let e = el; e && e !== document.documentElement; e = e.parentElement) {
      const cs = getComputedStyle(e);
      if (cs.display === 'none' || cs.visibility === 'hidden' || Number(cs.opacity) === 0) return false;
    }
    return true;
  };
  for (const p of spec.panels || []) {
    const el = document.querySelector(p);
    if (!el) { out.panels.push({ sel: p, present: false }); continue; }
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    out.panels.push({ sel: p, present: true, displayed: cs.display !== 'none' && cs.visibility !== 'hidden', w: r.width, h: r.height });
  }
  for (const e of spec.expect || []) {
    const all = [...document.querySelectorAll(e.sel)];
    // `equals`: only elements whose whole text is exactly that count.
    const visible = all.filter(shown).filter((v) => e.equals == null || v.textContent.trim() === e.equals);
    out.expect.push({ sel: e.sel, what: e.what, min: e.min ?? 1, count: visible.length });
  }
  if (!root) return out;
  const CONTROL = /^(button|input|select|textarea)$/;
  const index = new Map();
  const walk = [root, ...root.querySelectorAll('*')];
  const limit = spec.limit || 6000;
  for (const el of walk) {
    if (out.elements.length >= limit) { out.truncated = true; break; }
    if (el instanceof SVGElement && el.tagName.toLowerCase() !== 'svg') continue;
    if (/^(script|style|option|br|template)$/i.test(el.tagName)) continue;
    const cs = getComputedStyle(el);
    if (cs.display === 'none' || cs.visibility === 'hidden' || Number(cs.opacity) === 0) continue;
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) continue;
    // Walk every ancestor up to <body>: a hidden one hides this element, and
    // each one whose overflow is not visible narrows what can be seen of it.
    // A fixed box escapes the clipping of everything above it.
    let clip = null;
    let hard = null; // the same, from ancestors that CUT (hidden/clip), not scroll
    let scrolled = false; // a scrolling ancestor hides part of it: legitimate
    let hidden = false;
    let a = -1;
    if (cs.position !== 'fixed') {
      for (let e = el.parentElement; e && e !== document.body && e !== document.documentElement; e = e.parentElement) {
        if (a < 0 && index.has(e)) a = index.get(e);
        const c = getComputedStyle(e);
        if (c.display === 'none' || c.visibility === 'hidden' || Number(c.opacity) === 0) { hidden = true; break; }
        if (c.overflowX !== 'visible' || c.overflowY !== 'visible') {
          const ar = e.getBoundingClientRect();
          const own = [c.overflowX !== 'visible' ? ar.left : -1e9, c.overflowY !== 'visible' ? ar.top : -1e9,
            c.overflowX !== 'visible' ? ar.right : 1e9, c.overflowY !== 'visible' ? ar.bottom : 1e9];
          clip = clip ? [Math.max(clip[0], own[0]), Math.max(clip[1], own[1]), Math.min(clip[2], own[2]), Math.min(clip[3], own[3])] : own;
          const cuts = (v) => v === 'hidden' || v === 'clip';
          if (cuts(c.overflowX) || cuts(c.overflowY)) {
            const h = [cuts(c.overflowX) ? ar.left : -1e9, cuts(c.overflowY) ? ar.top : -1e9,
              cuts(c.overflowX) ? ar.right : 1e9, cuts(c.overflowY) ? ar.bottom : 1e9];
            hard = hard ? [Math.max(hard[0], h[0]), Math.max(hard[1], h[1]), Math.min(hard[2], h[2]), Math.min(hard[3], h[3])] : h;
          }
          const scrolls = (v) => v === 'auto' || v === 'scroll';
          if ((scrolls(c.overflowX) && (r.left < ar.left - 1 || r.right > ar.right + 1))
            || (scrolls(c.overflowY) && (r.top < ar.top - 1 || r.bottom > ar.bottom + 1))) scrolled = true;
          // An overflow:hidden box scrolled from script (a code view's line
          // gutter and highlight layer follow the textarea's scrollTop) is a
          // viewport too: it is scrolled, or holds over twice its own size.
          if (e.scrollTop > 0 || e.scrollLeft > 0 || e.scrollHeight > 2 * e.clientHeight + 4 || e.scrollWidth > 2 * e.clientWidth + 4) scrolled = true;
        }
        if (c.position === 'fixed') break;
      }
    } else {
      for (let e = el.parentElement; e && a < 0; e = e.parentElement) if (index.has(e)) a = index.get(e);
    }
    if (hidden) continue;
    let vis = [r.left, r.top, r.right, r.bottom];
    if (clip) vis = [Math.max(vis[0], clip[0]), Math.max(vis[1], clip[1]), Math.min(vis[2], clip[2]), Math.min(vis[3], clip[3])];
    const visible = vis[2] - vis[0] > 0.5 && vis[3] - vis[1] > 0.5;
    const tag = el.tagName.toLowerCase();
    const control = CONTROL.test(tag) || el.getAttribute('role') === 'button';
    let text = '';
    for (const n of el.childNodes) if (n.nodeType === 3) text += n.textContent;
    text = text.replace(/\s+/g, ' ').trim();
    // A one-line field shows its VALUE as its text, and scrolls it sideways
    // when the box is too narrow: "1.0" in a 30px box reads "1.0(".
    const field = tag === 'input' && /^(text|number|search|)$/.test(el.type || '');
    const item = {
      i: out.elements.length,
      parent: a,
      sel: describe(el),
      tag,
      rect: [r.left, r.top, r.width, r.height],
      vis: visible ? [vis[0], vis[1], vis[2] - vis[0], vis[3] - vis[1]] : null,
      control,
      text: text.slice(0, 60),
      fixed: cs.position === 'fixed',
    };
    if (text && (cs.overflowX === 'hidden' || cs.overflowX === 'clip')) {
      item.clipX = el.scrollWidth > el.clientWidth + 1;
      item.textOverflow = cs.textOverflow;
    }
    if (field && el.value && el !== document.activeElement) {
      item.text = String(el.value).slice(0, 60);
      item.clipX = el.scrollWidth > el.clientWidth + 1;
      item.textOverflow = cs.textOverflow;
    }
    if (text && (cs.overflowY === 'hidden' || cs.overflowY === 'clip')) {
      item.clipY = el.scrollHeight > el.clientHeight + 1;
    }
    // How much of a text box an ancestor that does not scroll cuts away: a
    // label sticking out of a fixed-height bar.
    if (text && hard && visible && !scrolled) {
      const lostX = r.width - Math.max(0, Math.min(r.right, hard[2]) - Math.max(r.left, hard[0]));
      const lostY = r.height - Math.max(0, Math.min(r.bottom, hard[3]) - Math.max(r.top, hard[1]));
      if (lostX > 2 || lostY > 2) item.cut = [Math.round(lostX), Math.round(lostY)];
    }
    if ((control || text) && visible) {
      const cx = (vis[0] + vis[2]) / 2;
      const cy = (vis[1] + vis[3]) / 2;
      const hit = cx >= 0 && cy >= 0 && cx < vw && cy < vh ? document.elementFromPoint(cx, cy) : null;
      item.top = !!hit && (hit === el || el.contains(hit));
    }
    index.set(el, item.i);
    out.elements.push(item);
  }
  return out;
}

// ---------------------------------------------------------------------------
// Layout checks (Node side, on what collectArea returned)
// ---------------------------------------------------------------------------

const area = (r) => Math.max(0, r[2]) * Math.max(0, r[3]);
function intersect(a, b) {
  const x0 = Math.max(a[0], b[0]);
  const y0 = Math.max(a[1], b[1]);
  const x1 = Math.min(a[0] + a[2], b[0] + b[2]);
  const y1 = Math.min(a[1] + a[3], b[1] + b[3]);
  return x1 > x0 && y1 > y0 ? [x0, y0, x1 - x0, y1 - y0] : null;
}

function ancestorOf(els, maybeAncestor, i) {
  for (let p = els[i].parent; p >= 0; p = els[p].parent) if (p === maybeAncestor) return true;
  return false;
}

/// Visible boxes that run past the window edge (by more than `slack` px).
/// Only the OUTERMOST offender of a nest is reported: a toolbar that runs off
/// the right edge drags all its buttons with it.
export function viewportOverflow(data, slack = 2) {
  const { vw, vh, elements: els } = data;
  const hits = new Set();
  const out = [];
  for (const e of els) {
    if (!e.vis) continue;
    const [x, y, w, h] = e.vis;
    const right = x + w - vw;
    const bottom = y + h - vh;
    const left = -x;
    const over = Math.max(right, bottom, left, -y);
    if (over <= slack) continue;
    let p = e.parent;
    while (p >= 0 && !hits.has(p)) p = els[p].parent;
    hits.add(e.i);
    if (p >= 0) continue;
    const sides = [];
    if (right > slack) sides.push(`${Math.round(right)}px past the right edge`);
    if (bottom > slack) sides.push(`${Math.round(bottom)}px past the bottom`);
    if (left > slack) sides.push(`${Math.round(left)}px past the left edge`);
    if (-y > slack) sides.push(`${Math.round(-y)}px above the top`);
    out.push({ kind: 'overflow', sel: e.sel, detail: sides.join(', ') });
  }
  return out;
}

/// Panels the area must show that are displayed yet have no size (a flex
/// child squeezed to 0, a dock with height 0), or are missing outright.
export function zeroSizePanels(data) {
  const out = [];
  for (const p of data.panels || []) {
    if (!p.present) out.push({ kind: 'missing-panel', sel: p.sel, detail: 'not in the page' });
    else if (!p.displayed) out.push({ kind: 'hidden-panel', sel: p.sel, detail: 'display:none / hidden while its area is open' });
    else if (p.w < 1 || p.h < 1) out.push({ kind: 'zero-size', sel: p.sel, detail: `${Math.round(p.w)}x${Math.round(p.h)}` });
  }
  return out;
}

/// Pairs of uncovered controls — or a control and a text label — whose
/// visible boxes overlap by more than `frac` of the smaller one: a title
/// running under a button. Nested pairs (a label inside its button) and
/// anything under a dialog (not `top`) are skipped. Two plain labels are not
/// checked (inline text runs legitimately share line boxes).
export function overlappingControls(data, frac = 0.3) {
  const els = data.elements;
  const ctl = els.filter((e) => (e.control || e.text) && e.vis && e.top && area(e.vis) >= 16);
  const out = [];
  for (let a = 0; a < ctl.length; a++) {
    for (let b = a + 1; b < ctl.length; b++) {
      const A = ctl[a];
      const B = ctl[b];
      if (!A.control && !B.control) continue;
      if (ancestorOf(els, A.i, B.i) || ancestorOf(els, B.i, A.i)) continue;
      const x = intersect(A.vis, B.vis);
      if (!x) continue;
      if (area(x) > frac * Math.min(area(A.vis), area(B.vis))) {
        out.push({ kind: 'overlap', sel: `${A.sel} / ${B.sel}`, detail: `${Math.round(x[2])}x${Math.round(x[3])} px shared` });
      }
    }
  }
  return out;
}

/// Text cut off with no ellipsis to say so: wider than its own clipping box,
/// wrapped taller than its own clipping box, or sticking out of an ancestor
/// that clips without scrolling (overflow hidden/clip).
export function clippedText(data) {
  const out = [];
  for (const e of data.elements) {
    if (!e.vis || !e.text) continue;
    if (e.clipX && e.textOverflow !== 'ellipsis') out.push({ kind: 'clipped-text', sel: e.sel, detail: `"${e.text}" is cut off with no ellipsis` });
    else if (e.clipY) out.push({ kind: 'clipped-text', sel: e.sel, detail: `"${e.text}" wraps taller than its box and is cut off` });
    else if (e.cut && (e.cut[0] > 2 || e.cut[1] > 2)) {
      const parts = [];
      if (e.cut[0] > 2) parts.push(`${e.cut[0]}px of its width`);
      if (e.cut[1] > 2) parts.push(`${e.cut[1]}px of its height`);
      out.push({ kind: 'clipped-text', sel: e.sel, detail: `"${e.text}" loses ${parts.join(' and ')} to an ancestor that does not scroll` });
    }
  }
  return out;
}

/// `expect` entries whose content count is below the minimum: a panel that
/// should list something and shows nothing.
export function emptyPanels(data) {
  return (data.expect || [])
    .filter((e) => e.count < e.min)
    .map((e) => ({ kind: 'empty', sel: e.sel, detail: `${e.what || 'content'}: ${e.count} shown, want at least ${e.min}` }));
}

/// Collapses findings that differ only by element (forty clipped tree rows
/// are one problem) into one entry with a count.
export function dedupe(findings) {
  const map = new Map();
  for (const f of findings) {
    const key = `${f.kind}|${f.sel}`;
    const prev = map.get(key);
    if (prev) prev.count += 1;
    else map.set(key, { ...f, count: 1 });
  }
  return [...map.values()];
}

/// Every layout check on one area's collected data.
export function layoutFindings(data) {
  if (data.missing) {
    return dedupe([{ kind: 'missing-root', sel: '(area root)', detail: 'the area did not render' }, ...zeroSizePanels(data), ...emptyPanels(data)]);
  }
  return dedupe([
    ...zeroSizePanels(data),
    ...emptyPanels(data),
    ...viewportOverflow(data),
    ...overlappingControls(data),
    ...clippedText(data),
  ]);
}

// ---------------------------------------------------------------------------
// Failure classification
// ---------------------------------------------------------------------------

// Requests that fail by design: a probe for an optional file.
const BENIGN_HTTP = [/^GET \/favicon/];

/// Splits one area's observations into `hard` (exit non-zero: something is
/// broken) and `soft` (worth a look: a heuristic, or a warning the editor
/// printed). Hard: an uncaught page exception, a console error, a request
/// answered 5xx or 4xx, a broken image, an area that did not open, a panel
/// that is missing/zero-size/empty when it must show something. Soft: console
/// warnings and the geometric layout heuristics (overflow, overlap, clipped
/// text), which can misfire on deliberate design.
export function classify(a) {
  const hard = [];
  const soft = [];
  if (a.error) hard.push(`could not open: ${a.error}`);
  for (const e of a.exceptions || []) hard.push(`page exception: ${e}`);
  for (const e of a.console_errors || []) hard.push(`console error: ${e}`);
  for (const w of a.console_warnings || []) soft.push(`console warning: ${w}`);
  for (const r of a.http_errors || []) {
    if (BENIGN_HTTP.some((re) => re.test(r.request))) continue;
    hard.push(`HTTP ${r.status}: ${r.request}`);
  }
  for (const b of a.broken_images || []) hard.push(`broken image: ${b.url} (${b.why})`);
  for (const f of a.layout || []) {
    const line = `${f.kind}: ${f.sel} — ${f.detail}${f.count > 1 ? ` (x${f.count})` : ''}`;
    if (['missing-root', 'missing-panel', 'hidden-panel', 'zero-size', 'empty'].includes(f.kind)) hard.push(line);
    else soft.push(line);
  }
  if (a.ready_ms != null && a.budget_ms != null && a.ready_ms > a.budget_ms) {
    soft.push(`slow: ${a.ready_ms} ms to become interactive (budget ${a.budget_ms} ms)`);
  }
  return { hard, soft };
}

// ---------------------------------------------------------------------------
// Picking what to open and select
// ---------------------------------------------------------------------------

/// `run/main_scene` from a project.godot's text (res:// or uid://), or null.
export function mainSceneFromProject(text) {
  let section = '';
  for (const raw of text.split('\n')) {
    const line = raw.trim();
    const sec = /^\[(.+)\]$/.exec(line);
    if (sec) { section = sec[1]; continue; }
    if (section !== 'application') continue;
    const m = /^run\/main_scene\s*=\s*"([^"]*)"/.exec(line);
    if (m) return m[1] || null;
  }
  return null;
}

// ---------------------------------------------------------------------------
// Picking what to select
// ---------------------------------------------------------------------------

const CONTROL_CLASSES = new Set(['Control', 'Label', 'Button', 'Panel', 'PanelContainer', 'TextureRect', 'ColorRect',
  'LineEdit', 'TextEdit', 'RichTextLabel', 'ProgressBar', 'TextureButton', 'CheckBox', 'CheckButton', 'OptionButton',
  'MenuButton', 'LinkButton', 'SpinBox', 'HSlider', 'VSlider', 'ScrollContainer', 'NinePatchRect', 'ItemList', 'Tree',
  'TabContainer', 'GraphEdit', 'SubViewportContainer', 'ReferenceRect', 'VideoStreamPlayer']);

export function nodeKind(cls) {
  if (CONTROL_CLASSES.has(cls) || /Container$|Box$|Slider$|ScrollBar$/.test(cls)) return 'control';
  if (/3D$/.test(cls) || cls === 'WorldEnvironment' || cls === 'GridMap') return '3d';
  if (/2D$/.test(cls) || cls === 'CanvasLayer' || cls === 'TileMap' || cls === 'TileMapLayer' || cls === 'ParallaxBackground') return '2d';
  return 'other';
}

/// Flattens /api/scene's nested `nodes` into a list in tree order.
export function flattenScene(tree) {
  const out = [];
  const walk = (n, depth) => {
    if (!n) return;
    out.push({ id: n.id, name: n.name, class: n.class, path: n.path, depth, has_script: !!n.has_script, is_instance: !!n.is_instance });
    for (const c of n.children || []) walk(c, depth + 1);
  };
  walk(tree, 0);
  return out;
}

/// Up to one node of each kind worth inspecting: 2D, 3D, Control, a scripted
/// node and an instanced scene, each a different node where possible, and
/// never the editor's synthetic root (depth 0). Shallow nodes win.
export function pickNodes(flat) {
  const nodes = flat.filter((n) => n.depth > 0);
  const used = new Set();
  const picks = [];
  const take = (role, pred) => {
    const n = nodes.find((x) => pred(x) && !used.has(x.id)) || nodes.find(pred);
    if (!n) return;
    used.add(n.id);
    picks.push({ role, ...n });
  };
  take('instanced scene', (n) => n.is_instance);
  take('scripted', (n) => n.has_script);
  take('2D', (n) => nodeKind(n.class) === '2d');
  take('3D', (n) => nodeKind(n.class) === '3d');
  take('Control', (n) => nodeKind(n.class) === 'control');
  return picks;
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

const esc = (s) => String(s).replace(/\|/g, '\\|').replace(/\n/g, ' ');

/// The markdown report: a summary table, then each area's findings.
export function renderMarkdown(report) {
  const lines = [];
  const hardN = report.areas.reduce((n, a) => n + a.hard.length, 0);
  const softN = report.areas.reduce((n, a) => n + a.soft.length, 0);
  lines.push(`# Editor tour: ${report.game}`);
  lines.push('');
  lines.push(`- editor: ${report.url} (build ${report.build || 'unknown'})`);
  lines.push(`- when: ${report.when}`);
  if (report.load_ms != null) lines.push(`- editor page interactive after ${report.load_ms} ms`);
  lines.push(`- verdict: **${hardN ? 'FAIL' : 'OK'}** — ${hardN} hard, ${softN} soft finding(s) across ${report.areas.length} areas`);
  if (report.editor_died) lines.push(`- **the editor died**, tour cut short: ${report.editor_died}`);
  if (report.contact_sheet) lines.push(`- contact sheet: ${report.contact_sheet}`);
  lines.push('');
  lines.push('| area | ready ms | hard | soft | screenshot |');
  lines.push('|---|---:|---:|---:|---|');
  for (const a of report.areas) {
    lines.push(`| ${esc(a.name)} | ${a.ready_ms ?? '-'} | ${a.hard.length} | ${a.soft.length} | ${a.screenshot ? esc(a.screenshot) : '-'} |`);
  }
  for (const a of report.areas) {
    if (!a.hard.length && !a.soft.length) continue;
    lines.push('');
    lines.push(`## ${a.name}`);
    for (const h of a.hard) lines.push(`- **HARD** ${h}`);
    for (const s of a.soft) lines.push(`- soft: ${s}`);
  }
  lines.push('');
  return lines.join('\n');
}

/// An HTML page laying the area screenshots out in a captioned grid, which the
/// tour renders in Chrome to make the contact-sheet PNG (no image library).
/// `shots` is [{ name, dataUrl, hard }].
export function contactSheetHtml(title, shots, cols = 4, thumbW = 400) {
  const cells = shots.map((s) => `<figure class="${s.hard ? 'bad' : ''}"><img src="${s.dataUrl}"><figcaption>${s.name.replace(/</g, '&lt;')}${s.hard ? ` — ${s.hard} hard` : ''}</figcaption></figure>`).join('');
  return `<!doctype html><html><head><meta charset="utf-8"><style>
body{margin:0;padding:12px;background:#1e1e22;color:#ddd;font:13px system-ui,sans-serif}
h1{font-size:15px;margin:0 0 10px}
.grid{display:grid;grid-template-columns:repeat(${cols},${thumbW}px);gap:10px}
figure{margin:0;border:2px solid #444;background:#111}
figure.bad{border-color:#e0443e}
img{display:block;width:${thumbW}px}
figcaption{padding:4px 6px}
</style></head><body><h1>${title.replace(/</g, '&lt;')}</h1><div class="grid">${cells}</div></body></html>`;
}

/// Page-wide problems (a title under a button) show in every area that scans
/// the whole page. Keeps each finding only in the FIRST area that saw it,
/// noting how many later areas saw it too, so the report lists it once.
/// Mutates and returns `areas`.
export function foldRepeats(areas) {
  const first = new Map(); // line -> { area, list, index, n }
  for (const a of areas) {
    for (const key of ['hard', 'soft']) {
      const keep = [];
      for (const line of a[key] || []) {
        const seen = first.get(line);
        if (seen) { seen.n += 1; continue; }
        first.set(line, { list: keep, index: keep.length, n: 0 });
        keep.push(line);
      }
      a[key] = keep;
    }
  }
  for (const [line, f] of first) if (f.n) f.list[f.index] = `${line} [also in ${f.n} later area${f.n > 1 ? 's' : ''}]`;
  return areas;
}

/// Exit status: 0 nothing hard, 1 hard findings, 2 the tour could not run.
export function exitCode(report) {
  if (report.fatal) return 2;
  return report.editor_died || report.areas.some((a) => a.hard.length) ? 1 : 0;
}
