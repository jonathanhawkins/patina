// Tests for scripts/lib/scene-audit-godot.mjs (the Godot side of scene-audit):
// `node --test scripts/tests/`.
//
// Every Godot and Patina line here is REAL output, not invented: Godot 4.6.1
// headless and patina-runner `--script-output` on one scene whose nodes each
// raise one error (an assert, a Nil access, an out-of-bounds index, a missing
// method, `get_node` on a missing path, `push_error`, a parse error, ...), and
// on games/truck_town's sub-scenes run standalone. The point of the diff is
// that the two engines word the same mistake differently, so the pairs below
// pin which wordings mean the same thing — and which do not.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import {
  parseGodotLog, godotError, patinaError, canonical, sameError, samePlace, diffScene, normalisePath, topLevel,
  godotSideKey, isShutdownNoise,
} from '../lib/scene-audit-godot.mjs';
import { applyGodot, sceneFindingsVsGodot } from '../scene-audit.mjs';
import { patchProjectGodot } from '../godot-reference.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const AUDIT = path.join(ROOT, 'scripts', 'scene-audit.mjs');

// Godot 4.6.1 `--headless` on the probe scene (stderr, verbatim).
const GODOT_PROBE = `Godot Engine v4.6.1.stable.official.14d19694e - https://godotengine.org

SCRIPT ERROR: Parse Error: Unexpected "Indent" in class body.
          at: GDScript::reload (res://broken.gd:4)
ERROR: Failed to load script "res://broken.gd" with error "Parse error".
   at: load (modules/gdscript/gdscript.cpp:2907)
SCRIPT ERROR: Assertion failed.
          at: _ready (res://main.gd:14)
          GDScript backtrace (most recent call first):
              [0] _ready (res://main.gd:14)
SCRIPT ERROR: Assertion failed: with message
          at: _ready (res://main.gd:15)
          GDScript backtrace (most recent call first):
              [0] _ready (res://main.gd:15)
SCRIPT ERROR: Invalid access to property or key 'position' on a base object of type 'Nil'.
          at: _ready (res://main.gd:16)
          GDScript backtrace (most recent call first):
              [0] _ready (res://main.gd:16)
SCRIPT ERROR: Cannot call method 'queue_free' on a null value.
          at: _ready (res://main.gd:17)
          GDScript backtrace (most recent call first):
              [0] _ready (res://main.gd:17)
SCRIPT ERROR: Out of bounds get index '3' (on base: 'Array')
          at: _ready (res://main.gd:18)
          GDScript backtrace (most recent call first):
              [0] _ready (res://main.gd:18)
SCRIPT ERROR: Invalid call. Nonexistent function 'no_such_method (via call)' in base 'Node2D (main.gd)'.
          at: _ready (res://main.gd:19)
          GDScript backtrace (most recent call first):
              [0] _ready (res://main.gd:19)
ERROR: Node not found: "Missing/Child" (relative to "/root/Main/E7").
   at: get_node (scene/main/node.cpp:1964)
   GDScript backtrace (most recent call first):
       [0] _ready (res://main.gd:20)
<Object#null>
SCRIPT ERROR: Invalid access to property or key 'nokey' on a base object of type 'Dictionary'.
          at: _ready (res://main.gd:21)
          GDScript backtrace (most recent call first):
              [0] _ready (res://main.gd:21)
SCRIPT ERROR: Division by zero error in operator '/'.
          at: _ready (res://main.gd:24)
          GDScript backtrace (most recent call first):
              [0] _ready (res://main.gd:24)
SCRIPT ERROR: Invalid call. Nonexistent function 'no_such_method_direct' in base 'Node2D (main.gd)'.
          at: _ready (res://main.gd:27)
          GDScript backtrace (most recent call first):
              [0] _ready (res://main.gd:27)
SCRIPT ERROR: Invalid access to property or key 'no_such_prop' on a base object of type 'Node2D (main.gd)'.
          at: _ready (res://main.gd:30)
          GDScript backtrace (most recent call first):
              [0] _ready (res://main.gd:30)
ERROR: custom error here
   at: push_error (core/variant/variant_utility.cpp:1024)
   GDScript backtrace (most recent call first):
       [0] _ready (res://main.gd:11)
WARNING: custom warning here
     at: push_warning (core/variant/variant_utility.cpp:1034)
     GDScript backtrace (most recent call first):
         [0] _ready (res://main.gd:12)
plain printerr line
ERROR: 6 resources still in use at exit (run with --verbose for details).
   at: clear (core/io/resource.cpp:810)
WARNING: ObjectDB instances leaked at exit (run with --verbose for details).
     at: cleanup (core/object/object.cpp:2641)
`;

// patina-runner --script-output on the same scene (script_errors, verbatim).
const PATINA_PROBE = [
  'custom error here\n   at: _ready:9',
  'WARNING: custom warning here\n   at: _ready:9',
  'plain printerr line',
  '/Main/Broken: script failed to load: res://broken.gd: parse error: unexpected token NEWLINE at line 3, col 22; expected :',
  '/root/Main/E1: _ready(): type error: assertion failed: assertion failed',
  '/root/Main/E2: _ready(): type error: assertion failed: with message',
  "/root/Main/E3: _ready(): type error: cannot access member 'position' on Nil",
  '/root/Main/E4: _ready(): type error: cannot call method on Nil',
  '/root/Main/E5: _ready(): type error: index out of bounds: 3 (length 0)',
  "/root/Main/E6: _ready(): method not found: 'call'",
  '/root/Main/E7: _ready(): type error: node not found: Missing/Child',
  "/root/Main/E8: _ready(): type error: undefined variable: 'nokey'",
  '/root/Main/E9: _ready(): type error: division by zero',
  "/root/Main/E10: _ready(): method not found: 'no_such_method_direct'",
];

const N = (name, cls, p, children = [], extra = {}) => ({ name, class: cls, path: p, children, ...extra });
const probeTree = (withScript) => N('root', 'Window', '/root', [
  N('Main', 'Node2D', '/root/Main', [
    N('Broken', 'Node', '/root/Main/Broken'),
    ...Array.from({ length: 11 }, (_, i) => N(`E${i + 1}`, 'Node2D', `/root/Main/E${i + 1}`, [], withScript ? { script: 'res://main.gd' } : {})),
  ], withScript ? { script: 'res://main.gd' } : {}),
]);

test('godot log: blocks with their script location; prints and engine-only lines kept apart', () => {
  const b = parseGodotLog(GODOT_PROBE);
  const msgs = b.map((x) => x.message);
  assert.ok(!msgs.includes('plain printerr line'), 'printerr is a plain line, not a block');
  assert.ok(!msgs.some((m) => m.startsWith('<Object')), 'a print after a block ends it');
  const assertBlock = b.find((x) => x.message === 'Assertion failed.');
  assert.deepEqual(assertBlock.script, { func: '_ready', file: 'res://main.gd', line: 14 });
  // An engine error raised FROM a script: the C++ `at:` is not the location,
  // the backtrace's frame 0 is.
  const nf = b.find((x) => x.message.startsWith('Node not found'));
  assert.deepEqual(nf.at, { func: 'get_node', file: 'scene/main/node.cpp', line: 1964 });
  assert.deepEqual(nf.script, { func: '_ready', file: 'res://main.gd', line: 20 });
  const push = b.find((x) => x.message === 'custom warning here');
  assert.equal(push.severity, 'warning');
  assert.deepEqual(push.script, { func: '_ready', file: 'res://main.gd', line: 12 });
  // A parse error's location is the `at:` line itself (GDScript::reload).
  assert.deepEqual(b.find((x) => x.message.startsWith('Parse Error')).script, { func: 'GDScript::reload', file: 'res://broken.gd', line: 4 });
  // Shutdown noise has no script location and is dropped from the diff.
  const leak = b.find((x) => x.message.startsWith('6 resources'));
  assert.equal(leak.script, null);
  assert.equal(godotError(leak), null);
  assert.ok(isShutdownNoise('ObjectDB instances leaked at exit (run with --verbose for details).'));
  assert.ok(!isShutdownNoise('Assertion failed.'));
});

test('godot log: ANSI colour and CRLF do not hide a block', () => {
  const b = parseGodotLog('\x1b[1;31mSCRIPT ERROR: Assertion failed.\x1b[0m\r\n          at: _ready (res://a.gd:3)\r\n');
  assert.equal(b.length, 1);
  assert.deepEqual(b[0].script, { func: '_ready', file: 'res://a.gd', line: 3 });
});

// Same meaning, different words: [Godot text, Patina text (after `<node>: <func>(): `), meaning].
const SAME = [
  ['Assertion failed.', 'type error: assertion failed: assertion failed', { kind: 'assert', subject: null }],
  ['Assertion failed: with message', 'type error: assertion failed: with message', { kind: 'assert', subject: 'with message' }],
  ["Invalid access to property or key 'position' on a base object of type 'Nil'.", "type error: cannot access member 'position' on Nil", { kind: 'access', subject: 'position' }],
  // truck_town vehicle.gd:52 `turbometer.value += delta`: Godot reads first, Patina reports the write.
  ["Invalid access to property or key 'value' on a base object of type 'Nil'.", "type error: cannot assign member 'value' on Nil", { kind: 'access', subject: 'value' }],
  ["Invalid access to property or key 'nokey' on a base object of type 'Dictionary'.", "type error: undefined variable: 'nokey'", { kind: 'access', subject: 'nokey' }],
  ["Out of bounds get index '3' (on base: 'Array')", 'type error: index out of bounds: 3 (length 0)', { kind: 'index', subject: '3' }],
  ["Invalid call. Nonexistent function 'no_such_method_direct' in base 'Node2D (main.gd)'.", "method not found: 'no_such_method_direct'", { kind: 'no-method', subject: 'no_such_method_direct' }],
  ['Node not found: "Missing/Child" (relative to "/root/Main/E7").', 'type error: node not found: Missing/Child', { kind: 'node-not-found', subject: 'Missing/Child' }],
  ["Division by zero error in operator '/'.", 'type error: division by zero', { kind: 'div-zero', subject: null }],
];

test('meaning: each engine\'s wording of one mistake normalises to the same kind and subject', () => {
  for (const [g, p, want] of SAME) {
    assert.deepEqual(canonical(g), want, `Godot: ${g}`);
    assert.deepEqual(canonical(p), want, `Patina: ${p}`);
  }
  // Patina's Nil call does not name the method: it agrees with any subject.
  assert.deepEqual(canonical('type error: cannot call method on Nil'), { kind: 'null-call', subject: null });
  assert.deepEqual(canonical("Cannot call method 'queue_free' on a null value."), { kind: 'null-call', subject: 'queue_free' });
  // `RenderingServer.foo` names the method `foo`, like Godot does.
  assert.deepEqual(canonical("method not found: 'RenderingServer.foo'"), { kind: 'no-method', subject: 'foo' });
  // Trailing source excerpts and line numbers are not meaning.
  assert.deepEqual(canonical("type error: cannot access member 'x' on Nil at line 7, column 3\n  | a.x"), { kind: 'access', subject: 'x' });
});

test('meaning: different subjects or kinds are NOT the same error', () => {
  assert.notDeepEqual(canonical("Invalid access to property or key 'a' on a base object of type 'Nil'."), canonical("cannot access member 'b' on Nil"));
  assert.notDeepEqual(canonical("Out of bounds get index '0' (on base: 'Array[Node]')"), canonical('index out of bounds: 3 (length 0)'));
  // Godot's `call("no_such_method")` vs Patina's `method not found: 'call'`: a real difference.
  assert.notDeepEqual(canonical("Invalid call. Nonexistent function 'no_such_method (via call)' in base 'Node2D (main.gd)'."), canonical("method not found: 'call'"));
  assert.notDeepEqual(canonical('Assertion failed.'), canonical('division by zero'));
  // An unknown message still matches itself across engines, numbers aside.
  assert.deepEqual(canonical('Something odd 12 happened.'), canonical('something odd 3 happened'));
});

test('patina lines: every runner shape, node paths normalised to /root', () => {
  const scriptOf = (p) => (p === '/root/Main/E3' ? 'res://main.gd' : null);
  const call = patinaError("/root/Main/E3: _ready(): type error: cannot access member 'position' on Nil", scriptOf);
  assert.deepEqual([call.kind, call.subject, call.func, call.script, call.node], ['access', 'position', '_ready', 'res://main.gd', '/root/Main/E3']);
  // A load failure is recorded before the node enters the tree: `/Main/...`.
  const load = patinaError(PATINA_PROBE[3]);
  assert.deepEqual([load.kind, load.script, load.node], ['script-load', 'res://broken.gd', '/root/Main/Broken']);
  const pe = patinaError(PATINA_PROBE[0]);
  assert.deepEqual([pe.kind, pe.subject, pe.func, pe.line, pe.severity], ['push_error', 'custom error here', '_ready', 9, 'error']);
  const pw = patinaError(PATINA_PROBE[1]);
  assert.deepEqual([pw.kind, pw.subject, pw.severity], ['push_warning', 'custom warning here', 'warning']);
  assert.equal(patinaError('plain printerr line').kind, 'printerr');
  assert.equal(normalisePath('/Main/Broken'), '/root/Main/Broken');
  assert.equal(normalisePath('/root/Main'), '/root/Main');
  assert.equal(normalisePath('/root'), '/root');
});

test('matching: same meaning needs the same script and function when both are known', () => {
  const g = godotError(parseGodotLog(GODOT_PROBE).find((b) => b.message === 'Assertion failed.'));
  const p = patinaError('/root/Main/E1: _ready(): type error: assertion failed: assertion failed', () => 'res://main.gd');
  assert.ok(sameError(p, g));
  assert.ok(!sameError({ ...p, func: '_process' }, g), 'another function');
  assert.ok(!sameError({ ...p, script: 'res://other.gd' }, g), 'another script');
  assert.ok(sameError({ ...p, script: null }, g), 'a node with no known script still matches');
  assert.ok(samePlace({ ...p, kind: 'no-method' }, g));
  assert.ok(!samePlace({ ...p, script: null }, g), 'place needs both script and function');
});

test('diff: the probe scene — what both raise, what only Godot raises, and a wrong error', () => {
  const d = diffScene({ patina: { ok: true, tree: probeTree(false), script_errors: PATINA_PROBE }, godot: { ok: true, tree: probeTree(true), log: GODOT_PROBE }, rootName: 'Main' });
  // 12 of the 14 Patina lines match a Godot error by meaning (printerr is
  // not an error in Godot, E6 is the wrong error).
  assert.equal(d.matched.length, 12, d.matched.join('\n'));
  assert.deepEqual(d.patina_only, []);
  assert.deepEqual(d.godot_only, ["Invalid access to property or key 'no_such_prop' on a base object of type 'Node2D (main.gd)'. (res://main.gd:30 _ready())"]);
  assert.equal(d.different.length, 1);
  assert.match(d.different[0], /method not found: 'call'.*Nonexistent function 'no_such_method \(via call\)'/);
  assert.deepEqual(d.printerr, ['plain printerr line']);
  assert.deepEqual(d.node_count, { patina: 13, godot: 13 });
  assert.deepEqual(d.godot_engine, [], 'shutdown noise is not reported');
  // The parse error and its "Failed to load script" are ONE failure, matched.
  assert.ok(d.matched.some((m) => m.includes('script failed to load: res://broken.gd')));
});

// games/truck_town/vehicles/tow_truck.tscn run standalone (real output of both).
const TOW_GODOT = `SCRIPT ERROR: Assertion failed.
          at: _ready (res://vehicles/vehicle.gd:22)
          GDScript backtrace (most recent call first):
              [0] _ready (res://vehicles/vehicle.gd:22)
SCRIPT ERROR: Invalid access to property or key 'value' on a base object of type 'Nil'.
          at: _physics_process (res://vehicles/vehicle.gd:52)
          GDScript backtrace (most recent call first):
              [0] _physics_process (res://vehicles/vehicle.gd:52)
`;
const TOW_PATINA = [
  "/root/TowTruck/Body: _physics_process(): type error: cannot assign member 'value' on Nil",
  "/root/TowTruck/Body: _physics_process(): type error: cannot assign member 'value' on Nil",
  '/root/TowTruck/Body: _ready(): type error: assertion failed: assertion failed',
];
const towTree = (script) => N('root', 'Window', '/root', [N('TowTruck', 'Node3D', '/root/TowTruck', [N('Body', 'VehicleBody3D', '/root/TowTruck/Body', [], script ? { script: 'res://vehicles/vehicle.gd' } : {})], { instance: 'res://vehicles/tow_truck.tscn' })]);

const auditOf = (extra = {}) => ({ scene: 'x.tscn', format: 'text', ok: true, error: null, node_count: 2, placeholders: [], script_errors: [], missing_methods: [], script_load_failures: [], unsupported: [], ...extra });

test('findings vs Godot: a standalone sub-scene\'s errors that Godot raises too are expected, not failures', () => {
  const s = applyGodot(auditOf({ script_errors: TOW_PATINA }), { ok: true, tree: towTree(true), log: TOW_GODOT }, { patinaTree: towTree(false), patinaErrors: TOW_PATINA, rootName: 'TowTruck' });
  assert.deepEqual(s.findings, []);
  assert.equal(s.matches_godot.length, 2);
  assert.equal(s.godot.node_count, 2);
});

test('findings vs Godot: an error only Patina raises, and one only Godot raises, ARE findings', () => {
  const extra = [...TOW_PATINA, "/root/TowTruck/Body: _ready(): method not found: 'RenderingServer.foo'"];
  const s = applyGodot(auditOf(), { ok: true, tree: towTree(true), log: TOW_GODOT + "SCRIPT ERROR: Out of bounds get index '0' (on base: 'Array[Node]')\n          at: set_mood (res://town/town_scene.gd:114)\n" }, { patinaTree: towTree(false), patinaErrors: extra, rootName: 'TowTruck' });
  assert.deepEqual(s.missing_methods, ['RenderingServer.foo']);
  assert.ok(s.findings.some((f) => f.startsWith("script error: /root/TowTruck/Body: _ready(): method not found: 'RenderingServer.foo'") && f.endsWith('(Godot does not raise it)')), s.findings.join('\n'));
  assert.ok(s.findings.includes("missed check: Godot raises Out of bounds get index '0' (on base: 'Array[Node]') (res://town/town_scene.gd:114 set_mood()); Patina does not"), s.findings.join('\n'));
  assert.equal(s.findings.length, 2);
});

test('findings vs Godot: node count, instance class, placeholders Godot shares, load errors both have', () => {
  const g = N('root', 'Window', '/root', [N('Main', 'Node2D', '/root/Main', [
    N('A', 'Area2D', '/root/Main/A', [N('Shape', 'CollisionShape2D', '/root/Main/A/Shape')], { instance: 'res://a.tscn' }),
    N('B', 'Node', '/root/Main/B', [], { instance: 'res://b.scn' }),
  ])]);
  const p = N('root', 'Window', '/root', [N('Main', 'Node2D', '/root/Main', [
    N('A', 'Node', '/root/Main/A'),
    N('B', 'Node', '/root/Main/B'),
    N('Extra', 'Node', '/root/Main/Extra', [N('Deep', 'Node', '/root/Main/Extra/Deep')]),
  ])]);
  const placeholders = [
    { path: '/root/Main/A', got: 'Node', finding: '/root/Main/A <res://a.tscn> became an empty Node (its root is a Area2D)' },
    { path: '/root/Main/B', got: 'Node', finding: '/root/Main/B <res://b.scn> became an empty Node' },
  ];
  const s = applyGodot(auditOf({ placeholders }), { ok: true, tree: g, log: '' }, { patinaTree: p, patinaErrors: [], rootName: 'Main' });
  assert.deepEqual(s.findings, [
    'placeholder: /root/Main/A <res://a.tscn> became an empty Node (its root is a Area2D)',
    'node count: Patina 5, Godot 4 (only in Patina: /root/Main/Extra; only in Godot: /root/Main/A/Shape)',
    'instance class: /root/Main/A <res://a.tscn> is a Node in Patina, a Area2D in Godot',
  ]);
  // Both fail to load: Godot does not run it either, so it is expected.
  const both = applyGodot(auditOf({ ok: false, error: "Error reading 'x.scn'", format: 'binary' }), { ok: false, error: 'Godot exited 1 before frame 3', tree: null, log: '' }, { patinaTree: null, patinaErrors: [], rootName: null });
  assert.deepEqual(both.findings, []);
  // Only Patina fails: a finding, with no error diff.
  const only = applyGodot(auditOf({ ok: false, error: "Error reading 'x.scn'", format: 'binary' }), { ok: true, tree: g, log: TOW_GODOT }, { patinaTree: null, patinaErrors: [], rootName: null });
  assert.deepEqual(only.findings, ["load error: Error reading 'x.scn' (binary file) (Godot loads it)"]);
  assert.deepEqual(topLevel(['/a/b', '/a', '/c/d', '/a/b/c']), ['/a', '/c/d']);
  assert.equal(typeof sceneFindingsVsGodot, 'function');
});

test('cache key: changes with the game files, the frames and the Godot version', () => {
  const base = { files: [['a.gd', '1']], frames: 3, godotVersion: '4.6.1', tool: 't' };
  const k = godotSideKey(base);
  assert.equal(k, godotSideKey({ ...base }));
  assert.notEqual(k, godotSideKey({ ...base, files: [['a.gd', '2']] }));
  assert.notEqual(k, godotSideKey({ ...base, frames: 4 }));
  assert.notEqual(k, godotSideKey({ ...base, godotVersion: '4.7' }));
  assert.notEqual(k, godotSideKey({ ...base, tool: 'u' }));
});

test('project patch: scene-audit registers ITS capture autoload first, and still lowers 4.7 to 4.6', () => {
  const { text } = patchProjectGodot('config_version=5\n\n[application]\nconfig/features=PackedStringArray("4.7", "Forward Plus")\n\n[autoload]\n\nGame="*res://game.gd"\n', { godotVersion: [4, 6], autoloadName: 'SceneAuditCapture', autoloadFile: '__scene_audit_capture.gd' });
  assert.match(text, /\[autoload\]\n\nSceneAuditCapture="\*res:\/\/__scene_audit_capture.gd"\nGame=/);
  assert.match(text, /PackedStringArray\("4\.6", "Forward Plus"\)/);
  assert.doesNotMatch(text, /GodotReferenceCapture/);
});

// --- the whole CLI against a fake runner AND a fake Godot ---

function fakeGame(patinaErrors, godotLog) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'scene-audit-godot-'));
  const game = path.join(dir, 'game');
  fs.mkdirSync(game);
  fs.writeFileSync(path.join(game, 'project.godot'), 'config_version=5\n');
  fs.writeFileSync(path.join(game, 'car.tscn'), '[gd_scene format=3]\n\n[node name="Car" type="Node3D"]\n');
  const runner = path.join(dir, 'fake-runner');
  fs.writeFileSync(runner, `#!/usr/bin/env node
const a = process.argv.slice(2);
if (a[0] === '--class-list') { console.log(JSON.stringify(['Node', 'Node3D', 'Window'])); process.exit(0); }
if (a[0].startsWith('--')) { console.error('Error: no scene'); process.exit(1); }
console.log(JSON.stringify({ tree: { name: 'root', class: 'Window', path: '/root', children: [{ name: 'Car', class: 'Node3D', path: '/root/Car', children: [] }] }, script_errors: ${JSON.stringify(patinaErrors)} }));
`);
  fs.chmodSync(runner, 0o755);
  // Answers --version and --import, and a scene run the way scene-audit-capture.gd does.
  const godot = path.join(dir, 'fake-godot');
  fs.writeFileSync(godot, `#!/usr/bin/env node
const fs = require('fs');
const a = process.argv.slice(2);
if (a[0] === '--version') { console.log('4.6.1.stable.fake'); process.exit(0); }
if (a.includes('--import')) process.exit(0);
fs.appendFileSync(${JSON.stringify(path.join(dir, 'godot-runs'))}, 'run\\n');
const out = a[a.indexOf('--scene-audit-out') + 1];
process.stderr.write(${JSON.stringify(godotLog)});
fs.writeFileSync(out, JSON.stringify({ tree: { name: 'root', class: 'Window', path: '/root', children: [{ name: 'Car', class: 'Node3D', path: '/root/Car', script: 'res://car.gd', children: [] }] } }));
`);
  fs.chmodSync(godot, 0o755);
  return { dir, game, runner, godot, runs: () => (fs.existsSync(path.join(dir, 'godot-runs')) ? fs.readFileSync(path.join(dir, 'godot-runs'), 'utf8').split('\n').filter(Boolean).length : 0) };
}

const CAR_GODOT = 'SCRIPT ERROR: Assertion failed.\n          at: _ready (res://car.gd:3)\n';

test('CLI: an error Godot raises too is OK (exit 0); --no-godot reports it as before (exit 1); the Godot side is cached', () => {
  const { dir, game, runner, godot, runs } = fakeGame(['/root/Car: _ready(): type error: assertion failed: assertion failed'], CAR_GODOT);
  const cache = path.join(dir, 'cache');
  try {
    const run = (...extra) => spawnSync(process.execPath, [AUDIT, game, '--runner', runner, '--godot', godot, '--godot-cache', cache, '--json', ...extra], { encoding: 'utf8' });
    const r = run();
    assert.equal(r.status, 0, r.stdout + r.stderr);
    const rep = JSON.parse(r.stdout);
    assert.equal(rep.ok, true);
    assert.equal(rep.godot.hit, false);
    assert.equal(rep.scenes[0].matches_godot.length, 1);
    assert.equal(runs(), 1);
    // Second run: the cache answers, Godot is not started again.
    const again = run();
    assert.equal(again.status, 0, again.stderr);
    assert.equal(JSON.parse(again.stdout).godot.hit, true);
    assert.equal(runs(), 1, 'cached: no second Godot run');
    const old = run('--no-godot');
    assert.equal(old.status, 1);
    assert.deepEqual(JSON.parse(old.stdout).scenes[0].findings, ['script error: /root/Car: _ready(): type error: assertion failed: assertion failed']);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('CLI: an error only Patina raises fails the audit against Godot', () => {
  const { dir, game, runner, godot } = fakeGame(["/root/Car: _process(): type error: cannot access member 'x' on Nil"], CAR_GODOT);
  try {
    const r = spawnSync(process.execPath, [AUDIT, game, '--runner', runner, '--godot', godot, '--godot-cache', path.join(dir, 'cache')], { encoding: 'utf8' });
    assert.equal(r.status, 1, r.stdout + r.stderr);
    assert.match(r.stdout, /script error: \/root\/Car: _process\(\): type error: cannot access member 'x' on Nil .*\(Godot does not raise it\)/);
    assert.match(r.stdout, /missed check: Godot raises Assertion failed\. \(res:\/\/car\.gd:3 _ready\(\)\); Patina does not/);
    assert.match(r.stdout, /→ FAIL$/m);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('CLI: no Godot binary falls back to the Patina-only audit with a warning', () => {
  const { dir, game, runner } = fakeGame([], '');
  try {
    const r = spawnSync(process.execPath, [AUDIT, game, '--runner', runner, '--godot', path.join(dir, 'nope'), '--godot-cache', path.join(dir, 'cache')], { encoding: 'utf8' });
    assert.equal(r.status, 0, r.stdout + r.stderr);
    assert.match(r.stdout, /warning: no Godot side \(Godot not found/);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
