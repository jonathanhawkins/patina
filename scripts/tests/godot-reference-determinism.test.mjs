// Real-Godot tests of the reference capture's random-number pinning
// (pat-y2ylj): `node --test scripts/tests/godot-reference-determinism.test.mjs`.
//
// They run Godot.app (skipped when it is missing) — about 25 s in all, so
// they live apart from the pure-logic suites.
//
// Before the pinning, Godot's own clock-seeded RNG made Dodge the Creeps
// record a different reference every run: the parity gate reported f258
// 10.414 -> 15.050 on a frame Patina drew pixel-identically, and the capture
// cache hid it until its key changed.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { runGodotReference, defaultGodot, ROOT } from '../godot-reference.mjs';
import { compareCaptures } from '../lib/godot-parity.mjs';

const HAVE_GODOT = fs.existsSync(defaultGodot());
const skip = HAVE_GODOT ? false : `no Godot at ${defaultGodot()}`;

/** Godot's PCG32 (`thirdparty/misc/pcg.cpp`, RandomPCG's default stream). */
function pcg(seed) {
  const M = (1n << 64n) - 1n;
  const inc = ((1442695040888963407n << 1n) | 1n) & M;
  let state = 0n;
  const next = () => {
    const old = state;
    state = (old * 6364136223846793005n + inc) & M;
    const x = Number((((old >> 18n) ^ old) >> 27n) & 0xffffffffn);
    const rot = Number(old >> 59n);
    return ((x >>> rot) | (x << ((-rot) & 31))) >>> 0;
  };
  next();
  state = (state + BigInt(seed)) & M;
  next();
  return next;
}

test('two captures of dodge_the_creeps record the same trace and the same pictures', { skip }, () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'dtc-determinism-'));
  try {
    const opts = { watch: ['/root/Main/Player:position', '/root/Main/MobPath/MobSpawnLocation:progress_ratio'] };
    const game = path.join(ROOT, 'games', 'dodge_the_creeps');
    const a = runGodotReference(game, { ...opts, out: path.join(tmp, 'a') });
    const b = runGodotReference(game, { ...opts, out: path.join(tmp, 'b') });
    const check = compareCaptures(a.outDir, b.outDir);
    assert.deepEqual(check, { stable: true, differences: [] });
    assert.ok(a.frames.length >= 3, 'the scenario takes pictures');
    assert.equal(a.trace.seed, 42);

    // The first mob's `progress_ratio = randf()` is Godot's third number for
    // seed 42 — the Player's Trail (GPUParticles2D) constructor takes two.
    // Patina's Rust tests (gdscene constructor_random_draws_match_godot,
    // gdeditor editor_play_spawns_dodge_mobs_where_godot_does) assert the same
    // number, so this pins the capture side of the contract.
    const ratios = a.trace.frames.map((f) => f.values['/root/Main/MobPath/MobSpawnLocation:progress_ratio']);
    const first = ratios.find((r) => r !== 0);
    const next = pcg(42);
    next();
    next();
    const third = Math.fround(Math.fround(next()) / Math.fround(4294967295));
    assert.ok(Math.abs(first - third) < 1e-9, `first spawn ratio ${first} must be draw #3 of seed 42 (${third})`);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

test('randomize() and RandomNumberGenerator are pinned by the rule Patina applies', { skip }, () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rng-pin-'));
  const game = path.join(tmp, 'game');
  try {
    fs.mkdirSync(game);
    fs.writeFileSync(path.join(game, 'project.godot'), 'config_version=5\n\n[application]\n\nconfig/name="rng pin"\nrun/main_scene="res://main.tscn"\n');
    fs.writeFileSync(path.join(game, 'main.tscn'), '[gd_scene format=3]\n\n[ext_resource type="Script" path="res://main.gd" id="1"]\n\n[node name="Main" type="Node"]\nscript = ExtResource("1")\n');
    fs.writeFileSync(path.join(game, 'main.gd'), [
      'extends Node',
      'var field_rng := RandomNumberGenerator.new()',
      'var field_seed := 0',
      'var after_randomize := 0',
      'var rng_seed := 0',
      'var rng_draw := 0',
      'var reseeded := 0',
      'func _ready():',
      '\tfield_seed = field_rng.seed',
      '\trandomize()',
      '\tafter_randomize = randi()',
      '\tvar r := RandomNumberGenerator.new()',
      '\trng_seed = r.seed',
      '\trng_draw = r.randi()',
      '\tr.randomize()',
      '\treseeded = r.seed',
      '',
    ].join('\n'));
    const props = ['field_seed', 'after_randomize', 'rng_seed', 'rng_draw', 'reseeded'];
    const run = (name) => runGodotReference(game, { watch: props.map((p) => `/root/Main:${p}`), keys: 'wait:50', out: path.join(tmp, name) });
    const at0 = (cap) => Object.fromEntries(props.map((p) => [p, cap.trace.frames[0].values[`/root/Main:${p}`]]));
    const got = at0(run('a'));
    // `gdcore::rng::pin_seed` (engine-rs/crates/gdcore/src/rng.rs): the k-th
    // randomize() seeds base + k, the j-th generator base + 1000000 + j.
    assert.deepEqual(got, {
      field_seed: 42 + 1000001,
      after_randomize: pcg(43)(),
      rng_seed: 42 + 1000002,
      rng_draw: pcg(42 + 1000002)(),
      reseeded: 42 + 1000003,
    });
    assert.deepEqual(at0(run('b')), got, 'and it repeats');
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

test('a key held by the timeline survives the window losing it (focus out / move)', { skip }, () => {
  // pat-kiuul: macOS Godot releases every pressed key when its window resigns
  // key status or MOVES (`godot_window_delegate.mm`). Another Godot window
  // taking the focus mid-capture dropped the platformer's held W, so two
  // captures of the same scenario disagreed from f96. Moving our own window
  // fires the same release, deterministically.
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'held-key-'));
  const game = path.join(tmp, 'game');
  try {
    fs.mkdirSync(game);
    fs.writeFileSync(path.join(game, 'project.godot'), 'config_version=5\n\n[application]\n\nconfig/name="held key"\nrun/main_scene="res://main.tscn"\n');
    fs.writeFileSync(path.join(game, 'main.tscn'), '[gd_scene format=3]\n\n[ext_resource type="Script" path="res://main.gd" id="1"]\n\n[node name="Main" type="Node"]\nscript = ExtResource("1")\n');
    fs.writeFileSync(path.join(game, 'main.gd'), [
      'extends Node',
      'var held_ticks := 0',
      'var frames := 0',
      'func _physics_process(_d):',
      '\tif Input.is_key_pressed(KEY_W):',
      '\t\theld_ticks += 1',
      'func _process(_d):',
      '\tframes += 1',
      '\tif frames == 6 or frames == 12:',
      '\t\tget_window().position += Vector2i(7, 3)',
      '',
    ].join('\n'));
    // W held for 18 frames after a 2-frame wait (ms at 60 fps).
    const cap = runGodotReference(game, { watch: ['/root/Main:held_ticks'], keys: 'wait:33,KeyW:300,wait:50', out: path.join(tmp, 'a') });
    const values = cap.trace.frames.map((s) => s.values['/root/Main:held_ticks']);
    const held = values[values.length - 1];
    // One physics tick per frame (60 ticks/s): W reads pressed on every tick it is held.
    assert.equal(held, 18, `W must read pressed on all 18 of its ticks, got ${held} (${values.join(',')})`);
    assert.ok(cap.trace.held_keys_reasserted >= 1, 'the move released the key and the capture pressed it again');
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
