// The Godot side of scripts/scene-audit.mjs: run each scene of a game in REAL
// Godot (Godot.app 4.6.1, headless) for the same frames as the Patina runner,
// and diff the two engines' script errors and scene trees, so the audit flags
// only what differs from Godot.
//
// Why: a sub-scene run STANDALONE raises errors in Godot too (truck_town's
// vehicle.gd:22 `assert(turbometer)` — the parent scene normally sets it). An
// audit that reports those as Patina failures cries wolf on every run.
//
// Pure logic (log parsing, error normalisation, the diff) is exported and
// unit-tested in scripts/tests/scene-audit-godot.test.mjs; `captureGodotSide`
// does the I/O (temp copy, import, one Godot run per scene, content-hash cache).

import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { hashGameFiles } from './godot-parity.mjs';
import { patchProjectGodot, pinRandomnessInProject, parseGodotVersion, defaultGodot } from '../godot-reference.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '..', '..');
const CAPTURE_GD = path.join(HERE, 'scene-audit-capture.gd');
const PIN_GD = path.join(HERE, 'godot-reference-capture.gd');
const AUTOLOAD_NAME = 'SceneAuditCapture';
const AUTOLOAD_FILE = '__scene_audit_capture.gd';
/** Bump when what a cached Godot side holds changes shape. */
const CACHE_FORMAT = 1;

const stripAnsi = (s) => String(s || '').replace(/\x1b\[[0-9;]*[A-Za-z]/g, '');

// ---------------------------------------------------------------------------
// Godot's stderr
// ---------------------------------------------------------------------------

/**
 * Godot's error/warning blocks from its stderr. A block is a header line
 *   `SCRIPT ERROR: <msg>` · `ERROR: <msg>` · `WARNING: <msg>` · `USER ERROR: …`
 * followed by indented `at: <func> (<file>:<line>)` and, for errors raised
 * from a script, a `GDScript backtrace` whose `[0] <func> (res://x.gd:N)` is
 * the script frame. Returns [{severity, header, message, at, script}] where
 * `script` = {func, file, line} is the first res:// location (the `at:` line
 * when it is a script, else backtrace frame 0), or null for an engine-only
 * message. Plain lines (print / printerr) are not blocks.
 */
export function parseGodotLog(text) {
  const out = [];
  let cur = null;
  const loc = (s) => {
    const m = /^(.+?) \((.+):(\d+)\)\s*$/.exec(s.trim());
    return m ? { func: m[1], file: m[2], line: Number(m[3]) } : null;
  };
  for (const raw of stripAnsi(text).split('\n')) {
    const line = raw.replace(/\r$/, '');
    const h = /^(SCRIPT ERROR|USER SCRIPT ERROR|USER ERROR|ERROR|SCRIPT WARNING|USER WARNING|WARNING): (.*)$/.exec(line);
    if (h) {
      cur = { severity: /WARNING/.test(h[1]) ? 'warning' : 'error', header: h[1], message: h[2].trim(), at: null, script: null };
      out.push(cur);
      continue;
    }
    if (!cur) continue;
    const at = /^\s+at: (.*)$/.exec(line);
    if (at) {
      cur.at = loc(at[1]);
      if (cur.at && cur.at.file.startsWith('res://') && !cur.script) cur.script = cur.at;
      continue;
    }
    if (/^\s+GDScript backtrace/.test(line)) continue;
    const bt = /^\s+\[(\d+)\] (.*)$/.exec(line);
    if (bt) {
      const l = loc(bt[2]);
      if (l && l.file.startsWith('res://') && !cur.script) cur.script = l;
      continue;
    }
    cur = null; // an unindented line ends the block
  }
  return out;
}

/** Messages Godot prints at shutdown about the process itself, not the scene. */
const SHUTDOWN_NOISE = [
  /resources still in use at exit/,
  /ObjectDB instances leaked at exit/,
  /^Leaked instance/,
  /^Orphan StringName/,
  /^\d+ RID(s)? of type .* (was|were) leaked/,
  /^Pages in use exist at exit/,
];

export const isShutdownNoise = (m) => SHUTDOWN_NOISE.some((re) => re.test(m));

/**
 * One Godot block -> a comparable error, or null when it is not a scene error
 * (shutdown noise). Parse errors and the `Failed to load script` that follows
 * them both become `script-load` of that script.
 */
export function godotError(b) {
  if (isShutdownNoise(b.message)) return null;
  const parse = /^Parse Error: (.*)$/.exec(b.message);
  const failed = /^Failed to load script "([^"]+)"/.exec(b.message);
  if (parse || failed) {
    const file = failed ? failed[1] : b.script?.file || null;
    return { engine: 'godot', severity: 'error', kind: 'script-load', subject: file, script: file, func: null, line: b.script?.line ?? null, text: b.message, scripted: true };
  }
  const pushed = b.at && /^push_(error|warning)$/.test(b.at.func) && !b.at.file.startsWith('res://');
  const c = pushed ? { kind: `push_${b.severity}`, subject: b.message } : canonical(b.message);
  return {
    engine: 'godot', severity: b.severity, kind: c.kind, subject: c.subject,
    script: b.script?.file || null, func: b.script?.func || null, line: b.script?.line ?? null,
    text: b.message, scripted: !!b.script,
  };
}

// ---------------------------------------------------------------------------
// Patina's script_errors
// ---------------------------------------------------------------------------

/** `/Main/Broken` (a path recorded before the node entered the tree) -> `/root/Main/Broken`. */
export function normalisePath(p) {
  if (!p) return p;
  if (p === '/root' || p.startsWith('/root/')) return p;
  return p.startsWith('/') ? `/root${p}` : `/root/${p}`;
}

/**
 * One runner `script_errors` line -> a comparable error. The runner's shapes:
 *   `<node>: <func>(): <error>`                      a failed script call
 *   `<node>: script failed to load: <res>: <error>`  a script that did not parse
 *   `<msg>\n   at: <func>:<line>`                    push_error
 *   `WARNING: <msg>\n   at: <func>:<line>`           push_warning
 *   anything else                                    printerr output
 * `scriptOf(nodePath)` names the script on a node (from either tree), so a
 * call error can be keyed on the script like Godot's are.
 */
export function patinaError(line, scriptOf = () => null) {
  const text = String(line);
  let m = /^(\S+): script failed to load: (\S+): (.*)$/s.exec(text);
  if (m) return { engine: 'patina', severity: 'error', kind: 'script-load', subject: m[2], script: m[2], func: null, line: null, node: normalisePath(m[1]), text, scripted: true };
  m = /^(WARNING: )?([\s\S]*?)\n\s+at: ([^\n:]+):(\d+)\s*$/.exec(text);
  if (m && !/^\/\S*: \w+\(\): /.test(text)) {
    const severity = m[1] ? 'warning' : 'error';
    return { engine: 'patina', severity, kind: `push_${severity}`, subject: m[2].trim(), script: null, func: m[3], line: Number(m[4]), node: null, text, scripted: true };
  }
  m = /^(\/\S*|<node \d+>): ([A-Za-z_][\w]*)\(\): ([\s\S]*)$/.exec(text);
  if (m) {
    const node = normalisePath(m[1]);
    const c = canonical(m[3]);
    return { engine: 'patina', severity: 'error', kind: c.kind, subject: c.subject, script: scriptOf(node), func: m[2], line: lineOf(m[3]), node, text, scripted: true };
  }
  return { engine: 'patina', severity: 'error', kind: 'printerr', subject: text.trim(), script: null, func: null, line: null, node: null, text, scripted: false };
}

const lineOf = (s) => {
  const m = /\bat line (\d+)/.exec(s);
  return m ? Number(m[1]) : null;
};

// ---------------------------------------------------------------------------
// Meaning, not text
// ---------------------------------------------------------------------------

/**
 * What an error MEANS, as {kind, subject}, from either engine's wording. The
 * same mistake reads differently in each:
 *   Godot  `Invalid access to property or key 'value' on a base object of type 'Nil'.`
 *   Patina `type error: cannot assign member 'value' on Nil`
 * -> {kind: 'access', subject: 'value'}. Line numbers, `type error:` prefixes,
 * trailing source excerpts and quoting are dropped. An unknown message keeps
 * its text with numbers and quotes normalised, so the same unknown message
 * from both engines still matches.
 */
export function canonical(message) {
  let s = String(message).split('\n')[0].trim();
  s = s.replace(/\s+at line \d+(, col(umn)? \d+)?.*$/, '');
  s = s.replace(/^(type error|runtime error|error): /i, '');
  const rules = [
    [/^Assertion failed(?:[.:]\s*(.*))?$/i, (m) => ({ kind: 'assert', subject: assertSubject(m[1]) })],
    [/^assertion failed: (?:assertion failed: )?(.*)$/i, (m) => ({ kind: 'assert', subject: assertSubject(m[1]) })],
    [/Invalid (?:access|assignment) (?:to|of) (?:property or key|index) '([^']+)'/, (m) => ({ kind: 'access', subject: m[1] })],
    [/Invalid (?:get|set) index '([^']+)'/, (m) => ({ kind: 'access', subject: m[1] })],
    [/cannot (?:access|assign|get|set) (?:member|property) '([^']+)'/i, (m) => ({ kind: 'access', subject: m[1] })],
    [/^undefined variable: '([^']+)'/i, (m) => ({ kind: 'access', subject: m[1] })],
    [/Cannot call method '([^']+)' on a null value/, (m) => ({ kind: 'null-call', subject: m[1] })],
    [/Attempt to call function '([^']+)' in base 'null instance'/, (m) => ({ kind: 'null-call', subject: m[1] })],
    [/cannot call method(?: '([^']+)')? on (?:Nil|null)/i, (m) => ({ kind: 'null-call', subject: m[1] || null })],
    [/Out of bounds (?:get|set) index '(-?\d+)'/, (m) => ({ kind: 'index', subject: m[1] })],
    [/index out of bounds: (-?\d+)/i, (m) => ({ kind: 'index', subject: m[1] })],
    [/Nonexistent function '([^' ]+)(?: \(via call\))?'/, (m) => ({ kind: 'no-method', subject: m[1] })],
    [/method not found: '(?:[\w]+\.)?([^'.]+)'/, (m) => ({ kind: 'no-method', subject: m[1] })],
    [/has no method '([^']+)'/, (m) => ({ kind: 'no-method', subject: m[1] })],
    [/^undefined function: '([^']+)'/i, (m) => ({ kind: 'no-method', subject: m[1] })],
    [/^Node not found: "([^"]+)"/, (m) => ({ kind: 'node-not-found', subject: m[1] })],
    [/^node not found: (\S+)/i, (m) => ({ kind: 'node-not-found', subject: m[1].replace(/^["']|["']$/g, '') })],
    [/division by zero/i, () => ({ kind: 'div-zero', subject: null })],
    [/modulo by zero/i, () => ({ kind: 'div-zero', subject: null })],
  ];
  for (const [re, f] of rules) {
    const m = re.exec(s);
    if (m) return f(m);
  }
  return { kind: 'other', subject: s.toLowerCase().replace(/["'`]/g, '').replace(/-?\d+(\.\d+)?/g, '#').replace(/[.\s]+$/, '').replace(/\s+/g, ' ') };
}

const assertSubject = (msg) => {
  const t = (msg || '').trim().replace(/\.$/, '');
  return t && !/^assertion failed$/i.test(t) ? t : null;
};

/** Do two subjects agree? A side that could not name one (Patina's `cannot call method on Nil`) agrees with any. */
const subjectsAgree = (a, b) => a == null || b == null || a === b;
const sameOrUnknown = (a, b) => a == null || b == null || a === b;

/** Same mistake in the same place, by meaning. */
export function sameError(p, g) {
  return p.kind === g.kind && p.severity === g.severity && subjectsAgree(p.subject, g.subject)
    && sameOrUnknown(p.script, g.script) && sameOrUnknown(p.func, g.func);
}

/** Same script and function (both known), different meaning: Patina raised the WRONG error there. */
export function samePlace(p, g) {
  return p.script != null && p.script === g.script && p.func != null && p.func === g.func;
}

// ---------------------------------------------------------------------------
// Trees
// ---------------------------------------------------------------------------

export function walkTree(n, f) {
  if (!n) return;
  f(n);
  for (const c of n.children || []) walkTree(c, f);
}

/** Path -> node for a tree whose nodes carry `path` (both engines' dumps do). */
export function indexTree(tree) {
  const m = new Map();
  walkTree(tree, (n) => { if (n.path) m.set(normalisePath(n.path), n); });
  return m;
}

/** The scene's own subtree among /root's children (autoloads first): by root name, else the last child. */
export function sceneRoot(tree, rootName) {
  const kids = tree?.children || [];
  return kids.find((c) => rootName && c.name === rootName) || kids[kids.length - 1] || null;
}

const countNodes = (n) => (n ? 1 + (n.children || []).reduce((a, c) => a + countNodes(c), 0) : 0);

// ---------------------------------------------------------------------------
// The diff
// ---------------------------------------------------------------------------

const describe = (e) => {
  const where = e.script ? ` (${e.script}${e.line != null && e.engine === 'godot' ? `:${e.line}` : ''}${e.func ? ` ${e.func}()` : ''})` : e.func ? ` (${e.func}())` : '';
  return `${e.engine === 'godot' ? e.text : e.text.split('\n')[0]}${e.engine === 'patina' && e.node ? ` [${e.node}]` : ''}${where}`;
};

/**
 * Diffs one scene between the engines. `patina` = {ok, error, tree, script_errors,
 * script_load_failures}; `godot` = {ok, error, tree, log}. Returns
 *   matched       errors both raise (expected: a standalone sub-scene, say)
 *   patina_only   errors Godot does not raise                     -> finding
 *   godot_only    errors Godot raises and Patina does not (a missed check) -> finding
 *   different     same script+function, a different error         -> finding
 *   node_count    {patina, godot} and only_in_patina / only_in_godot paths
 *   instance_classes  instanced nodes whose class differs from Godot's
 *   godot_engine  Godot messages with no script location (info only)
 */
export function diffScene({ patina, godot, rootName }) {
  const gTree = godot.tree || null;
  const gIndex = indexTree(gTree);
  const pIndex = indexTree(patina.tree);
  const scriptOf = (p) => gIndex.get(p)?.script || pIndex.get(p)?.script || null;
  const pErrs = (patina.script_errors || []).map((l) => patinaError(l, scriptOf));
  const blocks = parseGodotLog(godot.log || '');
  const gAll = blocks.map(godotError).filter(Boolean);
  const gErrs = gAll.filter((e) => e.scripted);
  const godot_engine = uniq(gAll.filter((e) => !e.scripted).map((e) => `${e.severity}: ${e.text}`));
  // printerr is not an error in Godot (a plain stderr line): it matches when
  // Godot printed the same line, and is never a finding.
  const plain = new Set(stripAnsi(godot.log || '').split('\n').map((l) => l.trim()));

  const matched = [], patina_only = [], different = [], printerr = [];
  const gHit = new Set();
  const pending = [];
  for (const p of pErrs) {
    if (p.kind === 'printerr') {
      printerr.push(`${p.subject}${plain.has(p.subject) ? '' : ' (Godot did not print it)'}`);
      continue;
    }
    const hits = gErrs.filter((g) => sameError(p, g));
    if (hits.length) {
      hits.forEach((g) => gHit.add(g));
      matched.push(`${describe(p)} == Godot: ${hits[0].text}${hits[0].script ? ` (${hits[0].script}:${hits[0].line})` : ''}`);
    } else pending.push(p);
  }
  // Patina's script-load failures that the runner only logged (older runner):
  for (const l of patina.script_load_failures || []) {
    const res = l.split(': ')[0];
    if (pErrs.some((p) => p.kind === 'script-load' && p.script === res)) continue;
    const g = gErrs.find((e) => e.kind === 'script-load' && (e.script === res || res.endsWith(`/${String(e.script).replace(/^res:\/\//, '')}`)));
    if (g) { gHit.add(g); matched.push(`script failed to load: ${l} == Godot: ${g.text}`); } else patina_only.push(`script failed to load: ${l}`);
  }
  for (const p of pending) {
    const g = gErrs.find((e) => !gHit.has(e) && samePlace(p, e));
    if (g) {
      gHit.add(g);
      different.push(`Patina raises "${p.text.split('\n')[0]}" [${p.node}] where Godot raises "${g.text}" (${g.script}:${g.line} ${g.func}())`);
    } else patina_only.push(describe(p));
  }
  // A Godot error is covered when any Patina error matched it; script-load
  // failures are keyed by script, so the parse error and the "Failed to load"
  // that follows it are one failure.
  const loadHit = new Set([...gHit].filter((g) => g.kind === 'script-load').map((g) => g.script));
  const godot_only = uniq(gErrs.filter((g) => !gHit.has(g) && !(g.kind === 'script-load' && loadHit.has(g.script)))
    .filter((g) => !(g.kind === 'script-load' && g.text.startsWith('Failed to load script') && gErrs.some((o) => o !== g && o.kind === 'script-load' && o.script === g.script && o.text.startsWith('Parse Error'))))
    .map(describe));

  const pScene = sceneRoot(patina.tree, rootName);
  const gScene = gTree ? sceneRoot(gTree, rootName) : null;
  const node_count = { patina: pScene ? countNodes(pScene) : null, godot: gScene ? countNodes(gScene) : null };
  const only_in_patina = [], only_in_godot = [], instance_classes = [];
  if (pScene && gScene) {
    const pi = indexTree(pScene), gi = indexTree(gScene);
    for (const p of pi.keys()) if (!gi.has(p)) only_in_patina.push(p);
    for (const [p, g] of gi) {
      const n = pi.get(p);
      if (!n) only_in_godot.push(p);
      else if (g.instance && n.class !== g.class) instance_classes.push(`${p} <${g.instance}> is a ${n.class} in Patina, a ${g.class} in Godot`);
    }
  }
  return {
    matched: uniq(matched), patina_only: uniq(patina_only), godot_only, different: uniq(different), printerr: uniq(printerr),
    node_count, only_in_patina: topLevel(only_in_patina), only_in_godot: topLevel(only_in_godot), instance_classes, godot_engine,
  };
}

const uniq = (xs) => [...new Set(xs)].sort();

/** Drop paths under another listed path: a missing subtree is reported once, at its root. */
export function topLevel(paths) {
  const s = [...new Set(paths)].sort();
  return s.filter((p) => !s.some((q) => q !== p && p.startsWith(`${q}/`)));
}

// ---------------------------------------------------------------------------
// Running Godot (with a content-hash cache)
// ---------------------------------------------------------------------------

export function defaultCacheDir() {
  return path.join(ROOT, '.playtest-gate', 'godot-cache');
}

/** The cache key: the game's files, the frame count, the Godot version and this tool. */
export function godotSideKey({ files, frames, godotVersion, tool }) {
  return crypto.createHash('sha256').update(JSON.stringify({ files, frames, godot: String(godotVersion).trim(), tool, format: CACHE_FORMAT })).digest('hex').slice(0, 16);
}

function toolHash() {
  return crypto.createHash('sha256').update([CAPTURE_GD, PIN_GD].map((f) => fs.readFileSync(f)).join('\0')).digest('hex');
}

function runGodot(godot, args, timeoutMs) {
  return new Promise((resolve) => {
    const p = spawn(godot, args, { stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '', stderr = '', timedOut = false;
    p.stdout.on('data', (d) => (stdout += d));
    p.stderr.on('data', (d) => (stderr += d));
    const t = setTimeout(() => { timedOut = true; p.kill('SIGKILL'); }, timeoutMs);
    p.on('error', (e) => { clearTimeout(t); resolve({ code: 127, stdout, stderr: String(e), timedOut }); });
    p.on('exit', (code, signal) => { clearTimeout(t); resolve({ code, signal, stdout, stderr, timedOut }); });
  });
}

async function pool(items, n, fn) {
  const out = new Array(items.length);
  let next = 0;
  await Promise.all(Array.from({ length: Math.min(n, items.length) }, async () => {
    while (next < items.length) {
      const i = next++;
      out[i] = await fn(items[i]);
    }
  }));
  return out;
}

/**
 * Runs every scene in Godot (or reads the cache). Returns
 * {key, hit, version, scenes: {rel: {ok, error, tree, log}}}; throws when
 * Godot is not there.
 */
export async function captureGodotSide(gameDir, scenes, { frames = 3, godot = defaultGodot(), cacheDir = defaultCacheDir(), jobs = 4, timeoutMs = 60000, log = () => {} } = {}) {
  if (!fs.existsSync(godot)) throw new Error(`Godot not found at ${godot} (set $GODOT, or run with --no-godot)`);
  const version = spawnSync(godot, ['--version'], { encoding: 'utf8' }).stdout?.trim() || 'unknown';
  const key = godotSideKey({ files: hashGameFiles(gameDir), frames, godotVersion: version, tool: toolHash() });
  const file = path.join(cacheDir, path.basename(gameDir), `scene-audit-${key}.json`);
  let cached = null;
  try { cached = JSON.parse(fs.readFileSync(file, 'utf8')); } catch { /* miss */ }
  if (cached && scenes.every((s) => cached.scenes?.[s])) return { key, hit: true, version, file, scenes: cached.scenes };

  log(`running ${scenes.length} scenes in Godot ${version} (cache miss, key ${key})`);
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), `scene-audit-${path.basename(gameDir)}-`));
  try {
    fs.cpSync(gameDir, tmp, { recursive: true, filter: (p) => { const r = path.relative(gameDir, p); return !(r === '.godot' || r.startsWith(`.godot${path.sep}`)); } });
    const project = fs.readFileSync(path.join(tmp, 'project.godot'), 'utf8');
    const patched = patchProjectGodot(project, { godotVersion: parseGodotVersion(version), autoloadName: AUTOLOAD_NAME, autoloadFile: AUTOLOAD_FILE });
    fs.writeFileSync(path.join(tmp, 'project.godot'), patched.text);
    fs.copyFileSync(CAPTURE_GD, path.join(tmp, AUTOLOAD_FILE));
    // The randomness rewrite calls the reference capture's static helpers;
    // the file is copied for them, not registered as an autoload.
    fs.copyFileSync(PIN_GD, path.join(tmp, '__godot_reference_capture.gd'));
    pinRandomnessInProject(tmp);
    const imp = spawnSync(godot, ['--headless', '--path', tmp, '--import'], { encoding: 'utf8', timeout: 600_000 });
    if (imp.status !== 0) log(`Godot import exited ${imp.status}`);
    const out = {};
    await pool(scenes, jobs, async (rel) => {
      const res = `res://${rel.split(path.sep).join('/')}`;
      const dump = path.join(tmp, `__scene_audit_${crypto.createHash('sha1').update(rel).digest('hex').slice(0, 10)}.json`);
      const r = await runGodot(godot, ['--headless', '--path', tmp, res, '--', '--scene-audit-out', dump, '--scene-audit-frames', String(frames)], timeoutMs);
      let tree = null;
      try { tree = JSON.parse(fs.readFileSync(dump, 'utf8')).tree; } catch { /* no dump */ }
      const logText = stripAnsi(`${r.stderr}\n${r.stdout}`).split(tmp).join('<project>');
      out[rel] = {
        ok: !!tree, tree,
        error: tree ? null : r.timedOut ? 'timed out' : `Godot exited ${r.code ?? r.signal} before frame ${frames}`,
        log: logText,
      };
    });
    fs.mkdirSync(path.dirname(file), { recursive: true });
    const tmpFile = `${file}.tmp-${process.pid}`;
    fs.writeFileSync(tmpFile, JSON.stringify({ key, version, frames, scenes: out }));
    fs.renameSync(tmpFile, file);
    return { key, hit: false, version, file, scenes: out };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}
