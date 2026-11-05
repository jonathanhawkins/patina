// Tests for scripts/scene-audit.mjs: `node --test scripts/tests/`.
//
// The audit decides which engine bugs an imported game gets filed for, so its
// parsing and its placeholder rule are pinned here against canned runner
// output and fake trees — no engine build needed.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import {
  parseRunnerResult, parseTscn, findPlaceholders, isPlaceholder, expectedRootType, missingMethods,
  unsupportedTypes, parseOutline, liveTreePlaceholders, placeholderCandidates, sceneFindings, dedupe, fileFormat,
} from '../scene-audit.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const AUDIT = path.join(ROOT, 'scripts', 'scene-audit.mjs');

const ESC = '\x1b';
// What the runner really prints (tracing, with colour) on a binary .scn.
const BINARY_STDERR = `${ESC}[2m2026-11-05T00:44:58.572811Z${ESC}[0m ${ESC}[32m INFO${ESC}[0m ${ESC}[2mpatina_runner${ESC}[0m${ESC}[2m:${ESC}[0m loading scene\nError reading 'stage/grid_map.scn': stream did not contain valid UTF-8\n`;

const node = (name, cls, p, children = [], extra = {}) => ({ name, class: cls, path: p, children, ...extra });

test('runner output: a load error is the runner\'s own line, colour and timestamp stripped', () => {
  const r = parseRunnerResult({ code: 1, stdout: '', stderr: BINARY_STDERR });
  assert.equal(r.ok, false);
  assert.equal(r.error, "Error reading 'stage/grid_map.scn': stream did not contain valid UTF-8");
  // The Error line wins over whatever the runner printed after it.
  const later = parseRunnerResult({ code: 1, stdout: '', stderr: `${BINARY_STDERR}note: re-run with RUST_LOG=debug\n` });
  assert.equal(later.error, "Error reading 'stage/grid_map.scn': stream did not contain valid UTF-8");
  const t = parseRunnerResult({ code: null, signal: 'SIGKILL', stdout: '', stderr: '', timedOut: true });
  assert.equal(t.error, 'timed out');
  const panic = parseRunnerResult({ code: 101, stdout: '', stderr: "thread 'main' panicked at src/x.rs:1:2:\nboom\n" });
  assert.match(panic.error, /panicked at/);
});

test('runner output: node count is the SCENE\'s, not an autoload that comes first', () => {
  const tree = node('root', 'Window', '/root', [
    node('Game', 'Node', '/root/Game'),
    node('Main', 'Node2D', '/root/Main', [node('A', 'Sprite2D', '/root/Main/A'), node('B', 'Node', '/root/Main/B')]),
  ]);
  const out = JSON.stringify({ frame_count: 3, tree, script_errors: ["/root/Main: _ready(): method not found: 'X.y'"] });
  const r = parseRunnerResult({ code: 0, stdout: out, stderr: '' }, 'Main');
  assert.equal(r.ok, true);
  assert.equal(r.node_count, 3);
  assert.deepEqual(r.script_errors, ["/root/Main: _ready(): method not found: 'X.y'"]);
  // Without a name the scene is the LAST child of /root (autoloads first).
  assert.equal(parseRunnerResult({ code: 0, stdout: out, stderr: '' }).node_count, 3);
});

const STAGE = `[gd_scene format=3]

[ext_resource type="Script" path="res://stage/stage.gd" id="1_s"]
[ext_resource type="PackedScene" path="res://stage/grid_map.scn" id="1_t0f53"]
[ext_resource type="PackedScene" path="res://ui/joystick.tscn" id="2_j"]
[sub_resource type="Environment" id="Env_1"]
[sub_resource type="FancyNewResource" id="F_1"]

[node name="Stage" type="Node3D"]
script = ExtResource("1_s")

[node name="GridMap" parent="." unique_id=1098206425 instance=ExtResource("1_t0f53")]

[node name="Hud" type="CanvasLayer" parent="."]

[node name="Virtual Joystick" parent="Hud" instance=ExtResource("2_j")]
`;

test('parseTscn: node paths under /root, instances, resource types', () => {
  const t = parseTscn(STAGE);
  assert.equal(t.root.name, 'Stage');
  assert.deepEqual(t.nodes.map((n) => n.path), ['/root/Stage', '/root/Stage/GridMap', '/root/Stage/Hud', '/root/Stage/Hud/Virtual Joystick']);
  assert.equal(t.nodes[1].instance, '1_t0f53');
  assert.equal(t.ext['1_t0f53'].path, 'res://stage/grid_map.scn');
  assert.deepEqual(t.sub.map((s) => s.type), ['Environment', 'FancyNewResource']);
});

test('placeholders: an instance that came out as a bare Node is flagged; real roots are not', () => {
  const t = parseTscn(STAGE);
  const expected = { 'res://stage/grid_map.scn': null, 'res://ui/joystick.tscn': 'Control' };
  const tree = node('root', 'Window', '/root', [node('Stage', 'Node3D', '/root/Stage', [
    node('GridMap', 'Node', '/root/Stage/GridMap'),
    node('Hud', 'CanvasLayer', '/root/Stage/Hud', [node('Virtual Joystick', 'Node', '/root/Stage/Hud/Virtual Joystick')]),
  ])]);
  const found = findPlaceholders(t, tree, (r) => expected[r]);
  assert.deepEqual(found.map((p) => p.path), ['/root/Stage/GridMap', '/root/Stage/Hud/Virtual Joystick']);
  assert.match(found[1].finding, /became an empty Node \(its root is a Control\)/);
  // Loaded properly: no findings.
  const good = node('root', 'Window', '/root', [node('Stage', 'Node3D', '/root/Stage', [
    node('GridMap', 'GridMap', '/root/Stage/GridMap'),
    node('Hud', 'CanvasLayer', '/root/Stage/Hud', [node('Virtual Joystick', 'Control', '/root/Stage/Hud/Virtual Joystick', [node('Base', 'TextureRect', '/root/Stage/Hud/Virtual Joystick/Base')])]),
  ])]);
  assert.deepEqual(findPlaceholders(t, good, (r) => expected[r]), []);
  // An instance missing altogether is reported too.
  const gone = node('root', 'Window', '/root', [node('Stage', 'Node3D', '/root/Stage', [node('Hud', 'CanvasLayer', '/root/Stage/Hud')])]);
  assert.match(findPlaceholders(t, gone, (r) => expected[r])[0].finding, /GridMap .* is missing from the loaded tree/);
});

test('isPlaceholder: a scene whose root IS a Node is not a placeholder; unknown roots need no children', () => {
  assert.equal(isPlaceholder(node('M', 'Node', '/root/M'), 'Node'), false);
  assert.equal(isPlaceholder(node('M', 'Node', '/root/M'), 'Area2D'), true);
  assert.equal(isPlaceholder(node('M', 'Node', '/root/M'), null), true);
  assert.equal(isPlaceholder(node('M', 'Node', '/root/M', [node('c', 'Node', '/root/M/c')]), null), false);
  assert.equal(isPlaceholder(node('M', 'Area2D', '/root/M'), 'Area2D'), false);
});

test('expectedRootType follows inherited scenes, knows imported models, gives up on binary', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'scene-audit-test-'));
  try {
    fs.writeFileSync(path.join(dir, 'base.tscn'), '[gd_scene format=3]\n\n[node name="Base" type="CharacterBody3D"]\n');
    fs.writeFileSync(path.join(dir, 'child.tscn'), '[gd_scene format=3]\n\n[ext_resource type="PackedScene" path="res://base.tscn" id="1"]\n\n[node name="Child" instance=ExtResource("1")]\n');
    fs.writeFileSync(path.join(dir, 'packed.scn'), Buffer.from('RSCC\x00\x01binary'));
    assert.equal(expectedRootType(dir, 'res://child.tscn'), 'CharacterBody3D');
    assert.equal(expectedRootType(dir, 'res://robot.glb'), 'Node3D');
    assert.equal(expectedRootType(dir, 'res://packed.scn'), null);
    assert.equal(expectedRootType(dir, 'res://missing.tscn'), null);
    assert.equal(fileFormat(fs.readFileSync(path.join(dir, 'packed.scn'))), 'binary-compressed');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('missing methods are pulled out of script errors, sorted and unique', () => {
  const errs = [
    "/root/Stage: _ready(): method not found: 'RenderingServer.get_current_rendering_method'",
    "/root/UI: _ready(): method not found: 'DisplayServer.is_touchscreen_available'",
    "/root/UI/J: _ready(): method not found: 'DisplayServer.is_touchscreen_available'",
    '/root/Main: _process(): type error: cannot divide Nil by Nil',
  ];
  assert.deepEqual(missingMethods(errs), ['DisplayServer.is_touchscreen_available', 'RenderingServer.get_current_rendering_method']);
  const s = { error: null, format: 'text', placeholders: [], missing_methods: missingMethods(errs), script_errors: dedupe(errs), unsupported: [] };
  const f = sceneFindings(s);
  // The missing-method lines are not repeated as generic script errors.
  assert.deepEqual(f.filter((x) => x.startsWith('script error')), ['script error: /root/Main: _process(): type error: cannot divide Nil by Nil']);
  assert.equal(f.filter((x) => x.startsWith('missing method')).length, 2);
});

test('unsupported types: what ClassDB lacks, file references exempt', () => {
  const t = parseTscn(STAGE);
  const classes = new Set(['Node3D', 'CanvasLayer', 'Environment']);
  assert.deepEqual(unsupportedTypes(t, classes), [{ type: 'FancyNewResource', as: ['sub_resource'] }]);
  assert.deepEqual(unsupportedTypes(t, null), [], 'no class list (old runner) means no verdict, not every type');
});

test('MCP outline parses into a tree with instance sources; a budget marker is ignored', () => {
  const text = [
    'Game (Node3D)',
    '  Stage (Node3D) [res://stage/stage.gd] <res://stage/stage.tscn>',
    '    GridMap (Node) <res://stage/grid_map.scn>',
    '    Label3D (Label3D)',
    '  Player (Node) <res://player/player.tscn> {player}',
    '  TouchScreenUI (CanvasLayer) [res://touch_screen_ui.gd] <res://touch_screen_ui/touch_screen_ui.tscn>',
    '    Virtual Joystick (Control) [res://j.gd] <res://j.tscn>',
    '  … (node budget reached)',
  ].join('\n');
  const t = parseOutline(text);
  assert.equal(t.path, '/root/Game');
  assert.deepEqual(t.children.map((c) => c.name), ['Stage', 'Player', 'TouchScreenUI']);
  assert.equal(t.children[0].children[0].instance, 'res://stage/grid_map.scn');
  assert.equal(t.children[2].children[0].path, '/root/Game/TouchScreenUI/Virtual Joystick');
  assert.equal(t.children[1].instance, 'res://player/player.tscn');
});

test('live JSON tree: childless Node instances are the candidates; sources decide the verdict', () => {
  const tree = node('root', 'Window', '/root', [node('Game', 'Node3D', '/root/Game', [
    node('Stage', 'Node3D', '/root/Game/Stage', [node('GridMap', 'Node', '/root/Game/Stage/GridMap', [], { is_instance: true })], { is_instance: true }),
    node('Player', 'Node', '/root/Game/Player', [], { is_instance: true }),
    node('Music', 'Node', '/root/Game/Music', [], { is_instance: true }),
    node('Plain', 'Node', '/root/Game/Plain'),
  ])]);
  assert.deepEqual(placeholderCandidates(tree), ['/root/Game/Stage/GridMap', '/root/Game/Player', '/root/Game/Music']);
  const src = { '/root/Game/Stage/GridMap': 'res://stage/grid_map.scn', '/root/Game/Player': 'res://player/player.tscn', '/root/Game/Music': 'res://music.tscn' };
  const roots = { 'res://player/player.tscn': 'CharacterBody3D', 'res://music.tscn': 'Node' };
  const found = liveTreePlaceholders(tree, (r) => roots[r] ?? null, (p) => src[p] ?? null);
  // Music's scene root really is a Node: a childless Node instance is fine there.
  assert.deepEqual(found.map((p) => p.path), ['/root/Game/Stage/GridMap', '/root/Game/Player']);
  assert.match(found[1].finding, /<res:\/\/player\/player\.tscn> became an empty Node \(its root is a CharacterBody3D\)/);
});

// --- the CLI, against a fake runner ----------------------------------------

function fakeGame(runnerBody) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'scene-audit-cli-'));
  const game = path.join(dir, 'game');
  fs.mkdirSync(path.join(game, 'sub'), { recursive: true });
  fs.writeFileSync(path.join(game, 'project.godot'), 'config_version=5\n');
  fs.writeFileSync(path.join(game, 'sub', 'child.tscn'), '[gd_scene format=3]\n\n[node name="Child" type="Area2D"]\n');
  fs.writeFileSync(path.join(game, 'main.tscn'), '[gd_scene format=3]\n\n[ext_resource type="PackedScene" path="res://sub/child.tscn" id="1"]\n\n[node name="Main" type="Node2D"]\n\n[node name="Child" parent="." instance=ExtResource("1")]\n');
  const runner = path.join(dir, 'fake-runner');
  fs.writeFileSync(runner, `#!/usr/bin/env node\n${runnerBody}\n`);
  fs.chmodSync(runner, 0o755);
  return { dir, game, runner };
}

// Answers --class-list, the flag probes, and a scene run by printing `tree`.
const RUNNER = (childClass) => `
const a = process.argv.slice(2);
if (a[0] === '--class-list') { console.log(JSON.stringify(['Area2D', 'Node', 'Node2D'])); process.exit(0); }
if (a[0].startsWith('--')) { console.error('Error: no scene'); process.exit(1); }
const scene = a[0];
const kids = scene === 'main.tscn' ? [{ name: 'Child', class: '${childClass}', path: '/root/Main/Child', children: [] }] : [];
const rootName = scene === 'main.tscn' ? 'Main' : 'Child';
const cls = scene === 'main.tscn' ? 'Node2D' : 'Area2D';
console.log(JSON.stringify({ tree: { name: 'root', class: 'Window', path: '/root', children: [{ name: rootName, class: cls, path: '/root/' + rootName, children: kids }] }, script_errors: [] }));
`;

test('CLI: a placeholder instance fails the audit (exit 1) and names the scene', () => {
  const { dir, game, runner } = fakeGame(RUNNER('Node'));
  try {
    const r = spawnSync(process.execPath, [AUDIT, game, '--runner', runner, '--json'], { encoding: 'utf8' });
    assert.equal(r.status, 1, r.stderr);
    const rep = JSON.parse(r.stdout);
    assert.equal(rep.ok, false);
    const main = rep.scenes.find((s) => s.scene === 'main.tscn');
    assert.deepEqual(main.findings, ['placeholder: /root/Main/Child <res://sub/child.tscn> became an empty Node (its root is a Area2D)']);
    assert.deepEqual(rep.scenes.map((s) => s.scene), ['main.tscn', path.join('sub', 'child.tscn')], 'sorted, sub-folders included');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('CLI: a clean game exits 0', () => {
  const { dir, game, runner } = fakeGame(RUNNER('Area2D'));
  try {
    const r = spawnSync(process.execPath, [AUDIT, game, '--runner', runner], { encoding: 'utf8' });
    assert.equal(r.status, 0, r.stdout + r.stderr);
    assert.match(r.stdout, /→ OK$/m);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
