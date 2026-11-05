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
import { runGodotReference, defaultGodot, ROOT, DisturbedCaptureError } from '../godot-reference.mjs';
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
    // The window can never be key, so it never RESIGNS key status — the
    // macOS event that releases every held key (Enter, held f1..f18, was
    // released at f1/f2 in 2 of 4 runs before the window was made unfocusable).
    for (const cap of [a, b]) {
      assert.deepEqual(cap.trace.disturbances, []);
      const windowFocusOut = cap.trace.window_events.filter((e) => e.what === 1005);
      assert.deepEqual(windowFocusOut, [], 'no NOTIFICATION_WM_WINDOW_FOCUS_OUT');
    }

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
    const cap = runGodotReference(game, { watch: ['/root/Main:held_ticks'], keys: 'wait:33,KeyW:300,wait:50', out: path.join(tmp, 'a'), acceptDisturbed: true });
    const values = cap.trace.frames.map((s) => s.values['/root/Main:held_ticks']);
    const held = values[values.length - 1];
    // One physics tick per frame (60 ticks/s): W reads pressed on every tick it is held.
    assert.equal(held, 18, `W must read pressed on all 18 of its ticks, got ${held} (${values.join(',')})`);
    assert.ok(cap.trace.held_keys_reasserted >= 1, 'the move released the key and the capture pressed it again');
    assert.ok(cap.trace.disturbances.some((d) => d.kind === 'held_key_released'), 'and the run is marked disturbed, so it is never taken as a reference');
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});

test('a capture a focus change disturbed is run again, and only an undisturbed run is the reference', { skip }, () => {
  // The coordinator's platformer pair: y 591.858 vs 593.608 from f154. Its
  // ArrowUp (jump) is held f151..f168; a focus change released it, the
  // capture pressed it again, and the re-press was a FRESH press —
  // is_action_just_pressed fired a second time, so the jump started over on
  // whichever tick the focus moved. The test releases the held keys at f154
  // the way `Input::release_pressed_events` does.
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'disturbed-'));
  const game = path.join(ROOT, 'games', 'platformer');
  try {
    const clean = runGodotReference(game, { out: path.join(tmp, 'clean') });
    assert.deepEqual(clean.trace.disturbances, [], 'an undisturbed run records no disturbance (every iteration drew)');
    assert.equal(clean.trace.attempts, 1);

    // The mechanism: a run released at f154 is NOT the timeline.
    const kept = runGodotReference(game, { out: path.join(tmp, 'kept'), simulateFocusLossAt: [154], acceptDisturbed: true });
    assert.deepEqual(kept.trace.disturbances.map((d) => [d.kind, d.frame, d.detail]), [['held_key_released', 154, 'Up']]);
    const diff = compareCaptures(clean.outDir, kept.outDir);
    assert.equal(diff.stable, false);
    assert.match(diff.differences[0], /^f155 \/root\/Game\/Level\/Player:position/, 'the re-press restarts the jump on the next frame');

    // With one attempt, a disturbed run is refused...
    assert.throws(
      () => runGodotReference(game, { out: path.join(tmp, 'refused'), simulateFocusLossAt: [154], attempts: 1 }),
      (e) => e instanceof DisturbedCaptureError && /held_key_released at f154 \(Up\)/.test(e.message),
    );
    // ...and by default it is run again, and the second run is the reference.
    const retried = runGodotReference(game, { out: path.join(tmp, 'retried'), simulateFocusLossAt: [154] });
    assert.equal(retried.trace.attempts, 2);
    assert.match(retried.notes.join('\n'), /1 disturbed attempt\(s\) discarded/);
    assert.deepEqual(compareCaptures(clean.outDir, retried.outDir), { stable: true, differences: [] });

    // An iteration Godot does not draw (an occluded window: platformer_3d's
    // pictures differed by up to 181 per channel with an identical trace)
    // leaves the previous frame in the texture; it is detected the same way.
    const undrawn = runGodotReference(game, { out: path.join(tmp, 'undrawn'), simulateUndrawnAt: [89], acceptDisturbed: true });
    assert.deepEqual(undrawn.trace.disturbances.map((d) => [d.kind, d.frame]), [['frame_not_drawn', 89]]);
    const redrawn = runGodotReference(game, { out: path.join(tmp, 'redrawn'), simulateUndrawnAt: [89] });
    assert.equal(redrawn.trace.attempts, 2);
    assert.deepEqual(compareCaptures(clean.outDir, redrawn.outDir), { stable: true, differences: [] });
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
