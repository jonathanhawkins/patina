#!/usr/bin/env node
// scene-audit — load every scene of an imported game through the release
// patina-runner and report what silently went wrong.
//
//   node scripts/scene-audit.mjs <game_dir> [--json] [--frames 3] [--port N]
//        [--runner PATH] [--jobs N] [--timeout-ms 30000]
//        [--no-godot] [--godot PATH] [--godot-cache DIR]
//
// For each .tscn/.scn under <game_dir> (dot-directories skipped) it runs
// `patina-runner <scene> --frames N --script-output` from the game directory
// and reports, per scene:
//
//   load error     the runner refused the scene (binary .scn, a parent path that
//                  does not exist, ...) — with the runner's own message
//   nodes          how many nodes the loaded scene has
//   placeholder    an `instance=ExtResource(...)` node that came out as an empty
//                  plain `Node` instead of the instanced scene's root — the
//                  loader swallowed a failure (grid_map.scn, player.tscn)
//   missing method a script call the engine does not have (RenderingServer.…),
//                  plus every other script error the scene raised
//   script load    a script that failed to read or PARSE, so its node ran with
//   failure        no script at all — from the runner's `script failed to load`
//                  script_errors (or, older runners, its WARN line on stderr)
//   unsupported    a node / ext_resource / sub_resource type Patina's ClassDB
//                  does not register (from `patina-runner --class-list`)
//
// The Godot side (default; --no-godot turns it off): every scene is also run
// in REAL Godot (Godot.app 4.6.1, headless, same frames) from a temp copy of
// the game (scripts/lib/scene-audit-godot.mjs), and the audit then reports
// only what DIFFERS from Godot:
//
//   script error     Patina raises it, Godot does not
//   missed check     Godot raises it, Patina does not
//   different error  same script + function, a different error
//   node count       the scene trees differ (with the paths only one side has)
//   instance class   an instanced root of another class than Godot's
//
// Errors both engines raise go under "matches Godot (expected)": a sub-scene
// run STANDALONE raises them in Godot too (truck_town's vehicle.gd:22
// `assert(turbometer)` — the parent scene normally sets it). Errors are
// matched by MEANING (assert / access 'x' on Nil / no method 'x' / ...),
// script and function, not by text: the two engines word them differently.
// Godot's side is cached by content hash (game files, frames, Godot version,
// capture script) in --godot-cache (default .playtest-gate/godot-cache), so a
// re-run is fast. No Godot found: a warning, and the Patina-only audit.
//
// --port N also reads a running editor (http://127.0.0.1:N): the open scene's
// edit tree, then a short Play (POST /api/runtime/play, ~1.5 s) for the LIVE
// tree and the game's errors from /api/logs, then stops it. Never on someone
// else's port: the Play replaces whatever they were running.
//
// Deterministic: scenes, findings and errors are sorted; timestamps dropped.
// Exit 0 all clean · 1 a finding · 2 could not run.

import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { captureGodotSide, diffScene } from './lib/scene-audit-godot.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DEFAULT_RUNNER = path.join(ROOT, 'engine-rs', 'target', 'release', 'patina-runner');

export function parseArgs(argv) {
  const o = { dir: null, json: false, frames: 3, port: null, runner: process.env.PATINA_RUNNER || DEFAULT_RUNNER, jobs: Math.min(8, os.cpus().length), timeoutMs: 30000, godot: true, godotPath: null, godotCache: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const v = () => {
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      return argv[++i];
    };
    if (a === '--json') o.json = true;
    else if (a === '--frames') o.frames = Number(v());
    else if (a === '--port') o.port = Number(v());
    else if (a === '--runner') o.runner = path.resolve(v());
    else if (a === '--jobs') o.jobs = Math.max(1, Number(v()));
    else if (a === '--timeout-ms') o.timeoutMs = Number(v());
    else if (a === '--no-godot') o.godot = false;
    else if (a === '--godot') o.godotPath = path.resolve(v());
    else if (a === '--godot-cache') o.godotCache = path.resolve(v());
    else if (a === '-h' || a === '--help') o.help = true;
    else if (a.startsWith('-')) throw new Error(`unknown option ${a}`);
    else if (!o.dir) o.dir = a;
    else throw new Error(`unexpected argument ${a}`);
  }
  if (!o.help && !o.dir) throw new Error('usage: scene-audit.mjs <game_dir> [--json] [--port N]');
  return o;
}

export const stripAnsi = (s) => String(s || '').replace(/\x1b\[[0-9;]*[A-Za-z]/g, '').replace(/\x1b\]8;;[^\x1b]*\x1b\\/g, '');

// ---------------------------------------------------------------------------
// Static reading of a scene file
// ---------------------------------------------------------------------------

/// The format of a resource file from its first bytes.
export function fileFormat(buf) {
  const head = buf.subarray(0, 4).toString('latin1');
  if (head === 'RSRC') return 'binary';
  if (head === 'RSCC') return 'binary-compressed';
  if (head === 'glTF') return 'glb';
  return 'text';
}

const attrs = (header) => {
  const out = {};
  for (const m of header.matchAll(/(\w+)=("(?:[^"\\]|\\.)*"|ExtResource\(\s*"[^"]*"\s*\)|\S+)/g)) {
    let val = m[2];
    if (val.startsWith('"')) val = val.slice(1, -1);
    out[m[1]] = val;
  }
  return out;
};

/// The sections of a text .tscn/.tres that matter here: ext_resources by id,
/// sub_resource types, and nodes (with the ext_resource they instance).
export function parseTscn(text) {
  const ext = {}, sub = [], nodes = [];
  for (const m of text.matchAll(/^\[(ext_resource|sub_resource|node)\s+([^\n]*)\]\s*$/gm)) {
    const a = attrs(m[2]);
    if (m[1] === 'ext_resource') ext[a.id] = { type: a.type, path: a.path };
    else if (m[1] === 'sub_resource') sub.push({ id: a.id, type: a.type });
    else {
      const inst = /^ExtResource\(\s*"([^"]*)"\s*\)$/.exec(a.instance || '');
      nodes.push({ name: a.name, type: a.type || null, parent: a.parent ?? null, instance: inst ? inst[1] : null });
    }
  }
  const root = nodes.find((n) => n.parent == null) || null;
  for (const n of nodes) {
    if (!root) n.path = null;
    else if (n === root) n.path = `/root/${root.name}`;
    else n.path = `/root/${root.name}${n.parent === '.' ? '' : `/${n.parent}`}/${n.name}`;
  }
  return { ext, sub, nodes, root };
}

const resPath = (gameDir, res) => (res && res.startsWith('res://') ? path.join(gameDir, res.slice(6)) : null);

/// What class an instanced scene's root should come out as, or null when that
/// cannot be told without loading it (binary scene, missing file).
export function expectedRootType(gameDir, res, depth = 0) {
  const file = resPath(gameDir, res);
  if (!file || depth > 8) return null;
  if (/\.(glb|gltf|fbx|blend|obj|dae)$/i.test(file)) return 'Node3D'; // Godot's imported-scene root
  let buf;
  try { buf = fs.readFileSync(file); } catch { return null; }
  if (fileFormat(buf) !== 'text') return null;
  const t = parseTscn(buf.toString('utf8'));
  if (!t.root) return null;
  if (t.root.type) return t.root.type;
  if (t.root.instance) return expectedRootType(gameDir, t.ext[t.root.instance]?.path, depth + 1); // inherited scene
  return null;
}

// ---------------------------------------------------------------------------
// The runner's verdict
// ---------------------------------------------------------------------------

/// One runner run → { ok, error, tree, node_count, script_errors }.
/// `rootName` picks the scene among /root's children: autoloads come first.
export function parseRunnerResult({ code, signal, stdout, stderr, timedOut }, rootName = null) {
  const err = stripAnsi(stderr).split('\n').map((l) => l.trim()).filter(Boolean);
  if (timedOut) return { ok: false, error: 'timed out', tree: null, node_count: null, script_errors: [] };
  let out = null;
  try { out = stdout && stdout.trim() ? JSON.parse(stdout) : null; } catch { /* not JSON */ }
  if (code !== 0 || !out) {
    // The runner's own failure line (`Error reading 'x': ...`), else a panic,
    // else the last line it printed. Tracing lines carry a timestamp: drop it.
    const line = err.find((l) => /^Error\b/.test(l))
      || err.find((l) => /panicked at|^thread '/.test(l))
      || err.filter((l) => !/^\d{4}-\d\d-\d\dT/.test(l)).pop()
      || err.pop()
      || `exited ${code ?? signal}`;
    return { ok: false, error: line.replace(/^\d{4}-\d\d-\d\dT\S+\s+/, ''), tree: out?.tree ?? null, node_count: null, script_errors: [] };
  }
  const kids = out.tree?.children || [];
  const scene = kids.find((c) => rootName && c.name === rootName) || kids[kids.length - 1] || null;
  const script_errors = out.script_errors || [];
  return {
    ok: true, error: null, tree: out.tree, node_count: scene ? countNodes(scene) : 0, script_errors,
    script_load_failures: scriptLoadFailures(script_errors, err),
  };
}

/// Scripts that never attached because they failed to read or parse, as
/// sorted, unique `<script>: <error>` strings. The runner reports them in
/// `script_errors` as `<node>: script failed to load: <res path>: <error>`;
/// an older runner only logs `WARN ... failed to parse GDScript path=<abs>
/// error=...` on stderr, which used to be the WHOLE signal — a script that
/// never loaded passed the audit as OK (pat-kkqnm). Both are read; a stderr
/// line whose path ends with an already-reported res path is the same failure.
export function scriptLoadFailures(scriptErrors, stderrLines = []) {
  const found = new Map();
  for (const e of scriptErrors) {
    const m = /: script failed to load: (\S+): (.*)$/.exec(e);
    if (m && !found.has(m[1])) found.set(m[1], m[2]);
  }
  for (const l of stderrLines) {
    const m = /failed to (?:parse|read) GDScript(?: file)?\s+path=(\S+)\s+error=(.*)$/.exec(l);
    if (!m) continue;
    const abs = m[1];
    const known = [...found.keys()].some((res) => abs.endsWith(`/${res.replace(/^res:\/\//, '')}`));
    if (!known && !found.has(abs)) found.set(abs, m[2].replace(/^"|"$/g, ''));
  }
  return [...found].map(([p, e]) => `${p}: ${e}`).sort();
}

export function countNodes(n) {
  return 1 + (n.children || []).reduce((a, c) => a + countNodes(c), 0);
}

export function findByPath(n, p) {
  if (!n) return null;
  if (n.path === p) return n;
  for (const c of n.children || []) {
    const hit = findByPath(c, p);
    if (hit) return hit;
  }
  return null;
}

/// Is a loaded instance node a stand-in for a scene that failed to load?
/// A childless plain `Node` where the instanced scene's root is something
/// else (or unknown — a binary scene) is the loader's silent fallback.
export function isPlaceholder(node, expected) {
  if (!node || node.class !== 'Node') return false;
  if (expected && expected !== 'Node') return true;
  return !expected && !(node.children || []).length;
}

/// Instance nodes of one scene that the runner loaded as placeholders.
export function findPlaceholders(parsed, tree, expectedOf) {
  const out = [];
  for (const n of parsed.nodes) {
    if (!n.instance || !n.path) continue;
    const res = parsed.ext[n.instance]?.path || `(ext_resource ${n.instance})`;
    const expected = expectedOf(res);
    const live = findByPath(tree, n.path);
    if (!live) out.push({ path: n.path, instance: res, expected, got: null, finding: `${n.path} <${res}> is missing from the loaded tree` });
    else if (isPlaceholder(live, expected)) {
      out.push({ path: n.path, instance: res, expected, got: live.class, finding: `${n.path} <${res}> became an empty ${live.class}${expected ? ` (its root is a ${expected})` : ''}` });
    }
  }
  return out;
}

/// The MCP `tree` outline (`props: false`) as a node tree. Each line is
/// `<indent>Name (Class) [script] <res://instanced.tscn> {groups}`, two spaces
/// per depth; the first line is the scene root, at `/root/<Name>`. The JSON
/// trees (/api/runtime/scene) do not carry the instance's source — the outline
/// does, and that is what names the scene that failed.
export function parseOutline(text) {
  const roots = [], stack = [];
  for (const line of String(text || '').split('\n')) {
    const m = /^( *)(.+?) \(([A-Za-z_][A-Za-z0-9_]*)\)(?: \[([^\]]*)\])?(?: <([^>]*)>)?(?: \{[^}]*\})?\s*$/.exec(line);
    if (!m) continue;
    const depth = m[1].length / 2;
    const node = { name: m[2], class: m[3], script: m[4] || null, instance: m[5] || null, children: [] };
    while (stack.length > depth) stack.pop();
    const parent = stack[stack.length - 1];
    node.path = parent ? `${parent.path}/${node.name}` : `/root/${node.name}`;
    (parent ? parent.children : roots).push(node);
    stack.push(node);
  }
  return roots[0] || null;
}

/// Instance nodes of a live JSON tree (/api/runtime/scene: `is_instance`,
/// `class`, `children`) that look like a failed load: a childless plain Node.
export function placeholderCandidates(tree) {
  const out = [];
  const walk = (n) => {
    if (!n) return;
    if (n.is_instance && n.class === 'Node' && !(n.children || []).length) out.push(n.path);
    for (const c of n.children || []) walk(c);
  };
  walk(tree);
  return out;
}

/// Placeholders among a live tree's instances. Each names the scene it was
/// instanced from in `instance_path`; `instanceOf(path)` is the fallback for an
/// editor too old to send it, or null when unknown.
export function liveTreePlaceholders(tree, expectedOf, instanceOf = () => null) {
  const out = [];
  const walk = (n) => {
    if (!n) return;
    if (n.is_instance) {
      const res = n.instance_path || instanceOf(n.path);
      const expected = res ? expectedOf(res) : null;
      if (isPlaceholder(n, expected)) {
        out.push({ path: n.path, instance: res, expected, got: n.class, finding: `${n.path}${res ? ` <${res}>` : ''} became an empty ${n.class}${expected ? ` (its root is a ${expected})` : ''}` });
      }
    }
    for (const c of n.children || []) walk(c);
  };
  walk(tree);
  return out;
}

/// The engine methods a game called that do not exist, from script errors
/// like `/root/Stage: _ready(): method not found: 'RenderingServer.foo'`.
export function missingMethods(errors) {
  const names = new Set();
  for (const e of errors) {
    const m = /method not found: '([^']+)'/.exec(e) || /[Nn]onexistent function '([^']+)'/.exec(e) || /has no method '([^']+)'/.exec(e);
    if (m) names.add(m[1]);
  }
  return [...names].sort();
}

/// Node, ext_resource and sub_resource types a ClassDB list does not have.
/// `Script`/`PackedScene` ext_resources are file references the loader
/// resolves itself, so a ClassDB without them is not a finding.
export function unsupportedTypes(parsed, classes) {
  if (!classes) return [];
  const seen = new Map();
  const note = (type, where) => {
    if (!type || classes.has(type)) return;
    if (!seen.has(type)) seen.set(type, new Set());
    seen.get(type).add(where);
  };
  for (const n of parsed.nodes) note(n.type, 'node');
  for (const e of Object.values(parsed.ext)) if (!['Script', 'PackedScene'].includes(e.type)) note(e.type, 'ext_resource');
  for (const s of parsed.sub) note(s.type, 'sub_resource');
  return [...seen].map(([type, w]) => ({ type, as: [...w].sort() })).sort((a, b) => a.type.localeCompare(b.type));
}

/// Collapse repeated errors to `message ×n`, sorted.
export function dedupe(lines) {
  const c = new Map();
  for (const l of lines) c.set(l, (c.get(l) || 0) + 1);
  return [...c].sort((a, b) => a[0].localeCompare(b[0])).map(([m, n]) => (n > 1 ? `${m} ×${n}` : m));
}

/// Everything wrong with one scene, as finding strings.
export function sceneFindings(s) {
  const f = [];
  if (s.error) f.push(`load error: ${s.error}${s.format !== 'text' ? ` (${s.format} file)` : ''}`);
  for (const p of s.placeholders) f.push(`placeholder: ${p.finding}`);
  for (const l of s.script_load_failures || []) f.push(`script load failure: ${l}`);
  for (const m of s.missing_methods) f.push(`missing method: ${m}`);
  for (const e of s.script_errors) {
    if (e.includes(': script failed to load: ')) continue; // reported above
    if (!s.missing_methods.some((m) => e.includes(`'${m}'`))) f.push(`script error: ${e}`);
  }
  for (const u of s.unsupported) f.push(`unsupported type: ${u.type} (${u.as.join(', ')})`);
  return f;
}

/**
 * Everything about one scene that DIFFERS from Godot, as finding strings
 * (`s.godot` = the Godot run, `s.diff` = {@link diffScene}). Errors both
 * engines raise are not findings: they are in `s.diff.matched`.
 */
export function sceneFindingsVsGodot(s) {
  const f = [];
  const g = s.godot, d = s.diff;
  if (s.error && g.ok) f.push(`load error: ${s.error}${s.format !== 'text' ? ` (${s.format} file)` : ''} (Godot loads it)`);
  // A placeholder Godot also has (a childless plain Node there) is not Patina's.
  for (const p of s.placeholders) if (!p.godot_same) f.push(`placeholder: ${p.finding}`);
  if (d) {
    for (const e of d.patina_only) {
      if (e.startsWith('script failed to load: ')) f.push(`script load failure: ${e.slice('script failed to load: '.length)} (Godot loads it)`);
      else f.push(`script error: ${e} (Godot does not raise it)`);
    }
    for (const e of d.different) f.push(`different error: ${e}`);
    for (const e of d.godot_only) f.push(`missed check: Godot raises ${e}; Patina does not`);
    const n = d.node_count;
    if (s.ok && g.ok && n.patina !== n.godot) {
      const parts = [];
      if (d.only_in_patina.length) parts.push(`only in Patina: ${d.only_in_patina.slice(0, 4).join(', ')}${d.only_in_patina.length > 4 ? `, … (${d.only_in_patina.length})` : ''}`);
      if (d.only_in_godot.length) parts.push(`only in Godot: ${d.only_in_godot.slice(0, 4).join(', ')}${d.only_in_godot.length > 4 ? `, … (${d.only_in_godot.length})` : ''}`);
      f.push(`node count: Patina ${n.patina}, Godot ${n.godot}${parts.length ? ` (${parts.join('; ')})` : ''}`);
    }
    for (const c of d.instance_classes) f.push(`instance class: ${c}`);
  }
  for (const u of s.unsupported) f.push(`unsupported type: ${u.type} (${u.as.join(', ')})`);
  return f;
}

/**
 * Attaches one scene's Godot run to its Patina audit: the diff, Godot's own
 * node count, missing methods restricted to the errors Godot does not raise,
 * and the findings recomputed against Godot. `patinaTree` and
 * `patinaErrors` are the runner's raw tree and script_errors.
 */
export function applyGodot(s, godotScene, { patinaTree, patinaErrors, rootName }) {
  s.godot = { ok: godotScene.ok, error: godotScene.error, node_count: null };
  if (!godotScene.ok && !s.ok) {
    s.diff = null;
    s.matches_godot = [`load error: Godot does not run it either (${godotScene.error})`];
    s.findings = sceneFindingsVsGodot(s);
    return s;
  }
  if (!s.ok) {
    // Patina never ran the scene: its errors cannot be compared, the load
    // error is the finding.
    s.diff = null;
    s.matches_godot = [];
    s.findings = sceneFindingsVsGodot(s);
    return s;
  }
  const d = diffScene({
    patina: { ok: s.ok, tree: patinaTree, script_errors: patinaErrors, script_load_failures: s.script_load_failures },
    godot: godotScene, rootName,
  });
  s.diff = d;
  s.godot.node_count = d.node_count.godot;
  s.godot.engine_messages = d.godot_engine;
  s.matches_godot = d.matched;
  s.printerr = d.printerr;
  // Missing methods are only the ones Godot HAS (it raised nothing there).
  s.missing_methods = missingMethods([...d.patina_only, ...d.different]);
  const gIndex = new Map();
  const walk = (n) => { if (!n) return; gIndex.set(n.path, n); (n.children || []).forEach(walk); };
  walk(godotScene.tree);
  for (const p of s.placeholders) {
    const gn = gIndex.get(p.path);
    p.godot_same = !!gn && gn.class === p.got && !(gn.children || []).length;
  }
  s.findings = sceneFindingsVsGodot(s);
  return s;
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

export function listScenes(dir) {
  const out = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      if (e.name.startsWith('.')) continue;
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (/\.(tscn|scn)$/.test(e.name)) out.push(path.relative(dir, p));
    }
  };
  walk(dir);
  return out.sort();
}

function runRunner(runner, cwd, args, timeoutMs) {
  return new Promise((resolve) => {
    const p = spawn(runner, args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
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

export function classList(runner) {
  const r = spawnSync(runner, ['--class-list'], { encoding: 'utf8', timeout: 30000 });
  try {
    const list = JSON.parse(r.stdout);
    return Array.isArray(list) ? new Set(list) : null;
  } catch {
    return null;
  }
}

/// Which of the audit's runner flags this binary knows: an older build answers
/// `unknown argument: --x`, and the audit then runs without that check.
export function runnerFlags(runner) {
  const knows = (args) => !/unknown argument/.test(spawnSync(runner, args, { encoding: 'utf8', timeout: 30000 }).stderr || '');
  return { scriptOutput: knows(['--script-output']), resRoot: knows(['--res-root', '.']) };
}

async function auditScene(o, gameDir, rel, classes, flags) {
  const buf = fs.readFileSync(path.join(gameDir, rel));
  const format = fileFormat(buf);
  const parsed = format === 'text' ? parseTscn(buf.toString('utf8')) : { ext: {}, sub: [], nodes: [], root: null };
  const args = [rel, '--frames', String(o.frames)];
  if (flags.scriptOutput) args.push('--script-output');
  // `res://` is the project, not the scene's folder: without this every
  // instance in a sub-folder scene reads as a placeholder (runner default).
  if (flags.resRoot) args.push('--res-root', gameDir);
  const r = parseRunnerResult(await runRunner(o.runner, gameDir, args, o.timeoutMs), parsed.root?.name);
  const expectedCache = new Map();
  const expectedOf = (res) => {
    if (!expectedCache.has(res)) expectedCache.set(res, expectedRootType(gameDir, res));
    return expectedCache.get(res);
  };
  const script_errors = dedupe(r.script_errors);
  // Binary resources a text scene references: not a finding by themselves
  // (the loader may read them), but the first suspect when a node is bare.
  const binary_resources = Object.values(parsed.ext).map((e) => e.path).filter((p) => {
    const f = resPath(gameDir, p);
    try { return f && fileFormat(fs.readFileSync(f)) !== 'text' && !/\.(glb|png|jpe?g|webp|svg|wav|ogg|mp3|ttf|otf|woff2?)$/i.test(f); } catch { return false; }
  }).sort();
  const s = {
    scene: rel, format, ok: r.ok, error: r.error, node_count: r.node_count,
    placeholders: r.ok ? findPlaceholders(parsed, r.tree, expectedOf) : [],
    script_errors, missing_methods: missingMethods(r.script_errors),
    script_load_failures: r.script_load_failures || [],
    unsupported: unsupportedTypes(parsed, classes),
    binary_resources,
  };
  s.findings = sceneFindings(s);
  // The raw run, for the Godot diff (kept out of --json).
  Object.defineProperty(s, 'raw', { value: { tree: r.tree, script_errors: r.script_errors, rootName: parsed.root?.name || null }, enumerable: false });
  return s;
}

async function auditLive(port, gameDir) {
  const base = `http://127.0.0.1:${port}`;
  const get = async (p) => (await fetch(base + p)).json();
  const mcp = async (name, args) => {
    const r = await (await fetch(`${base}/mcp`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name, arguments: args } }),
    })).json();
    const text = (r.result?.content || []).filter((b) => b.type === 'text').map((b) => b.text).join('\n');
    if (r.error || r.result?.isError) throw new Error(`MCP ${name}: ${text || JSON.stringify(r.error)}`);
    return text;
  };
  const live = { port, findings: [] };
  const expectedOf = (res) => expectedRootType(gameDir, res);
  let started = false;
  try {
    const st = await get('/api/runtime/status');
    // Errors from here on are this run's (a second of clock slack).
    const t0 = Date.now() - 1000;
    if (st.running !== true) {
      await fetch(`${base}/api/runtime/play`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
      started = true;
    }
    let running = st.running === true;
    for (let i = 0; i < 50 && !running; i++) {
      await new Promise((r) => setTimeout(r, 100));
      running = (await get('/api/runtime/status')).running === true;
    }
    if (!running) live.findings.push('Play did not start in the editor');
    else {
      await new Promise((r) => setTimeout(r, 1500));
      // The whole live tree from the JSON route; each instance carries its
      // source scene in `instance_path`. Only an editor too old to send it
      // needs each suspect's source from a one-node MCP outline.
      const rt = await get('/api/runtime/scene');
      const scene = (rt.tree?.children || []).slice(-1)[0] || null;
      live.runtime_node_count = scene ? countNodes(scene) : null;
      const sources = new Map();
      const named = new Set();
      const collectNamed = (n) => {
        if (!n) return;
        if (n.instance_path) named.add(n.path);
        for (const c of n.children || []) collectNamed(c);
      };
      collectNamed(rt.tree);
      for (const p of placeholderCandidates(rt.tree).filter((p) => !named.has(p))) {
        const rel = scene && p.startsWith(`${scene.path}/`) ? p.slice(scene.path.length + 1) : '.';
        try { sources.set(p, parseOutline(await mcp('tree', { running: true, props: false, depth: 0, node: rel }))?.instance || null); } catch { sources.set(p, null); }
      }
      live.placeholders = liveTreePlaceholders(rt.tree, expectedOf, (p) => sources.get(p) ?? null);
      for (const p of live.placeholders) live.findings.push(`live tree placeholder: ${p.finding}`);
      const logs = await get('/api/logs');
      const errs = (Array.isArray(logs) ? logs : logs.entries || []).filter((e) => e.level === 'error' && (e.timestamp ?? 0) >= t0).map((e) => e.message);
      live.game_errors = dedupe(errs);
      live.missing_methods = missingMethods(errs);
      for (const m of live.missing_methods) live.findings.push(`game missing method: ${m}`);
      for (const e of live.game_errors) if (!live.missing_methods.some((m) => e.includes(`'${m}'`))) live.findings.push(`game error: ${e}`);
    }
  } catch (e) {
    live.error = `could not read the editor at ${base}: ${e.message}`;
    live.findings.push(live.error);
  } finally {
    if (started) await fetch(`${base}/api/runtime/stop`, { method: 'POST' }).catch(() => {});
  }
  live.findings = [...new Set(live.findings)].sort();
  return live;
}

export function printReport(rep) {
  const lines = [`scene-audit ${rep.game_dir}: ${rep.scenes.length} scenes, ${rep.frames} frames (${rep.runner})`];
  if (rep.warnings.length) lines.push(...rep.warnings.map((w) => `  warning: ${w}`));
  const w = Math.max(...rep.scenes.map((s) => s.scene.length), 10);
  for (const s of rep.scenes) {
    const tag = s.findings.length ? 'FAIL' : 'OK  ';
    const what = (s.ok ? `${s.node_count} nodes` : 'did not load') + (s.godot ? (s.godot.ok ? ` (Godot ${s.godot.node_count})` : ` (Godot: ${s.godot.error})`) : '');
    lines.push(`  ${tag}  ${s.scene.padEnd(w)}  ${what}`);
    for (const f of s.findings) lines.push(`          - ${f}`);
    if (s.matches_godot?.length) {
      lines.push(`          matches Godot (expected): ${s.matches_godot.length}`);
      for (const m of s.matches_godot.slice(0, 4)) lines.push(`            = ${m}`);
      if (s.matches_godot.length > 4) lines.push(`            = … ${s.matches_godot.length - 4} more (--json lists all)`);
    }
    if (s.binary_resources?.length) lines.push(`          note: ${s.binary_resources.length} binary resource(s): ${s.binary_resources.slice(0, 3).join(', ')}${s.binary_resources.length > 3 ? ', …' : ''}`);
  }
  if (rep.live) {
    lines.push(`  live editor :${rep.live.port}${rep.live.runtime_node_count != null ? ` (${rep.live.runtime_node_count} nodes playing)` : ''}`);
    for (const f of rep.live.findings) lines.push(`          - ${f}`);
    if (!rep.live.findings.length) lines.push('          (clean)');
  }
  const t = rep.totals;
  const vs = rep.godot ? ` · ${t.missed_checks} missed checks · ${t.different_errors} different errors · ${t.node_count_diffs} tree diffs · ${t.matches_godot} errors match Godot` : '';
  if (rep.godot) lines.push(`godot: ${rep.godot.version}, ${rep.godot.hit ? 'cached' : 'fresh run'} (${rep.godot.file})`);
  lines.push(`summary: ${t.scenes} scenes · ${t.load_errors} load errors · ${t.placeholders} placeholder instances · ${t.script_load_failures} script load failures · ${t.missing_methods} missing methods · ${t.script_errors} other script errors · ${t.unsupported} unsupported types${vs}${rep.live ? ` · ${rep.live.findings.length} live findings` : ''} → ${rep.ok ? 'OK' : 'FAIL'}`);
  return lines.join('\n');
}

export function totals(scenes, live) {
  const uniq = (xs) => new Set(xs).size;
  return {
    scenes: scenes.length,
    load_errors: scenes.filter((s) => s.findings.some((f) => f.startsWith('load error'))).length,
    placeholders: scenes.reduce((a, s) => a + s.findings.filter((f) => f.startsWith('placeholder')).length, 0),
    script_load_failures: uniq(scenes.flatMap((s) => s.findings.filter((f) => f.startsWith('script load failure')).map((f) => f.split(': ')[1]))),
    missing_methods: uniq(scenes.flatMap((s) => s.missing_methods)),
    script_errors: scenes.reduce((a, s) => a + s.findings.filter((f) => f.startsWith('script error')).length, 0),
    unsupported: uniq(scenes.flatMap((s) => s.unsupported.map((u) => u.type))),
    live_findings: live ? live.findings.length : 0,
    missed_checks: scenes.reduce((a, s) => a + s.findings.filter((f) => f.startsWith('missed check')).length, 0),
    different_errors: scenes.reduce((a, s) => a + s.findings.filter((f) => f.startsWith('different error')).length, 0),
    node_count_diffs: scenes.filter((s) => s.findings.some((f) => f.startsWith('node count') || f.startsWith('instance class'))).length,
    matches_godot: scenes.reduce((a, s) => a + (s.matches_godot || []).length, 0),
  };
}

async function main() {
  let o;
  try { o = parseArgs(process.argv.slice(2)); } catch (e) { console.error(`scene-audit: ${e.message}`); process.exit(2); }
  if (o.help) {
    const lines = fs.readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').slice(1);
    console.log(lines.slice(0, lines.findIndex((l) => !l.startsWith('//'))).map((l) => l.replace(/^\/\/ ?/, '')).join('\n'));
    process.exit(0);
  }
  const gameDir = path.resolve(o.dir);
  if (!fs.existsSync(path.join(gameDir, 'project.godot'))) { console.error(`scene-audit: no project.godot in ${gameDir}`); process.exit(2); }
  if (!fs.existsSync(o.runner)) { console.error(`scene-audit: no runner at ${o.runner} (build: scripts/rust_task.sh build --release -p patina-runner)`); process.exit(2); }
  const warnings = [];
  const classes = classList(o.runner);
  if (!classes) warnings.push('the runner has no --class-list (stale build?): unsupported types not checked');
  const flags = runnerFlags(o.runner);
  if (!flags.scriptOutput) warnings.push('the runner has no --script-output (stale build?): script errors not checked');
  if (!flags.resRoot) warnings.push('the runner has no --res-root (stale build?): instances in sub-folder scenes will read as placeholders');
  const rels = listScenes(gameDir);
  const scenes = await pool(rels, o.jobs, (rel) => auditScene(o, gameDir, rel, classes, flags));
  let godot = null;
  if (o.godot) {
    try {
      const g = await captureGodotSide(gameDir, rels, {
        frames: o.frames, ...(o.godotPath ? { godot: o.godotPath } : {}), ...(o.godotCache ? { cacheDir: o.godotCache } : {}),
        jobs: Math.min(4, o.jobs), log: (m) => console.error(`scene-audit: ${m}`),
      });
      godot = { version: g.version, key: g.key, hit: g.hit, file: path.relative(process.cwd(), g.file) };
      for (const s of scenes) applyGodot(s, g.scenes[s.scene], { patinaTree: s.raw.tree, patinaErrors: s.raw.script_errors, rootName: s.raw.rootName });
    } catch (e) {
      warnings.push(`no Godot side (${e.message}): every script error is reported, including ones Godot raises too`);
    }
  }
  const live = o.port ? await auditLive(o.port, gameDir) : null;
  const t = totals(scenes, live);
  const rep = {
    game_dir: path.relative(process.cwd(), gameDir) || '.', runner: path.relative(process.cwd(), o.runner), frames: o.frames, warnings,
    ok: scenes.every((s) => !s.findings.length) && !(live && live.findings.length), totals: t, godot, scenes, live,
  };
  console.log(o.json ? JSON.stringify(rep, null, 2) : printReport(rep));
  process.exit(rep.ok ? 0 : 1);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
