// Tests for the Godot-reference tools' pure logic (pat-xggzs):
// `node --test scripts/tests/godot-reference.test.mjs`.
//
// The whole point of the tools is that Godot and Patina replay the SAME
// frames, so the timeline translation is pinned frame by frame, and the diff
// metrics are pinned on hand-computable inputs.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  keyName, parseTimeline, toMcpInputs, pickShotFrames, parseWatch, numbersOf,
  parseMcpProbes, traceFromGodot, alignTraces, diffTrace, imageDiff, makeImage,
  resizeImage, encodePng, decodePng, sideBySide, verdict, heatColour,
} from '../lib/godot-reference-lib.mjs';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { patchProjectGodot, projectSetting, parseGodotVersion, resPath, pinRandomnessInScript, resolveScenario, DEFAULT_SEED, withUnfocusableWindow } from '../godot-reference.mjs';
import { mcpPlayArgs, screenshotFrames } from '../compare-to-godot.mjs';
import zlib from 'node:zlib';

// --- timeline translation ---------------------------------------------------

test('DOM codes and Godot names map to the one name both engines accept', () => {
  assert.equal(keyName('KeyW'), 'W');
  assert.equal(keyName('w'), 'W');
  assert.equal(keyName('Keyq'), 'Q');
  assert.equal(keyName('Space'), 'Space');
  assert.equal(keyName('ArrowLeft'), 'Left');
  assert.equal(keyName('Left'), 'Left');
  assert.equal(keyName('Digit3'), '3');
  assert.equal(keyName('ShiftLeft'), 'Shift');
  assert.equal(keyName('F5'), 'F5');
  assert.throws(() => keyName('Lft'), /unknown key "Lft"/);
});

test('a hold occupies exactly its frames, starting after the waits before it', () => {
  // wait:500 = 30 frames (1..30 idle), KeyW:1500 = 90 frames (31..120),
  // Space:300 = 18 frames (121..138), wait:1000 = 60 more (139..198).
  const t = parseTimeline('wait:500,KeyW:1500,Space:300,wait:1000', 60);
  assert.equal(t.frames, 198);
  assert.deepEqual(t.inputs.map(({ key, start, end }) => ({ key, start, end })), [
    { key: 'W', start: 31, end: 120 },
    { key: 'Space', start: 121, end: 138 },
  ]);
  assert.deepEqual(t.boundaries.map((b) => b.frame), [30, 120, 138, 198]);
});

test('a hold at t=0 starts on frame 1, never frame 0', () => {
  const t = parseTimeline('KeyA:100', 60);
  assert.deepEqual(t.inputs.map((i) => [i.start, i.end]), [[1, 6]]);
  assert.equal(t.frames, 6);
});

test('ms are rounded to the nearest frame, and a very short hold still lasts one frame', () => {
  const t = parseTimeline('KeyA:10,wait:25,KeyB:24', 60);
  // 10 ms = 0.6 -> 1 frame; 25 ms = 1.5 -> 2 frames; 24 ms = 1.44 -> 1 frame.
  assert.deepEqual(t.inputs.map((i) => [i.key, i.start, i.end]), [['A', 1, 1], ['B', 4, 4]]);
});

test('down/up overlap two keys, and the up closes on the frame before the cursor', () => {
  const t = parseTimeline('down:KeyD,wait:500,Space:100,up:KeyD,wait:200', 60);
  const by = Object.fromEntries(t.inputs.map((i) => [i.key, [i.start, i.end]]));
  assert.deepEqual(by.D, [1, 36]);
  assert.deepEqual(by.Space, [31, 36]);
  assert.equal(t.frames, 48);
});

test('a key left down is released at the end of the timeline', () => {
  const t = parseTimeline('down:ArrowRight,wait:1000', 60);
  assert.deepEqual(t.inputs.map((i) => [i.key, i.start, i.end]), [['Right', 1, 60]]);
});

test('the fps changes the frame count, not the shape', () => {
  const t = parseTimeline('wait:500,KeyW:1500', 30);
  assert.deepEqual(t.inputs.map((i) => [i.start, i.end]), [[16, 60]]);
});

test('bad steps are refused with the step named', () => {
  assert.throws(() => parseTimeline('KeyW'), /bad step "KeyW"/);
  assert.throws(() => parseTimeline('KeyW:abc'), /bad duration/);
  assert.throws(() => parseTimeline('up:KeyW'), /is not down/);
  assert.throws(() => parseTimeline('down:KeyW,down:KeyW'), /already down/);
});

test('the MCP inputs carry the same inclusive frames the Godot capture presses', () => {
  const t = parseTimeline('wait:500,KeyW:1500,Space:300', 60);
  assert.deepEqual(toMcpInputs(t), [
    { key: 'W', at: 31, to: 120 },
    { key: 'Space', at: 121, to: 138 },
  ]);
});

test('picture frames are the step boundaries, thinned to the MCP limit of 8 keeping the last', () => {
  const t = parseTimeline('wait:500,KeyW:1500,Space:300,wait:1000', 60);
  assert.deepEqual(pickShotFrames(t, t.frames), [30, 120, 138, 198]);
  assert.deepEqual(pickShotFrames(t, 240), [30, 120, 138, 198, 240]);
  const many = parseTimeline(Array.from({ length: 20 }, () => 'KeyA:100').join(','), 60);
  const shots = pickShotFrames(many, many.frames, 8);
  assert.equal(shots.length, 8);
  assert.equal(shots[0], 6);
  assert.equal(shots.at(-1), many.frames);
  assert.ok(shots.every((f, i) => i === 0 || f > shots[i - 1]));
});

test('the MCP play arguments probe every frame plus frame 0 and picture the same frames', () => {
  const t = parseTimeline('wait:500,KeyW:1500', 60);
  const args = mcpPlayArgs({ frames: t.frames, timeline: t, watches: [parseWatch('/root/Game/Player:position')], shots: [30, 120], size: [640, 360] });
  assert.deepEqual(args.probes, [
    { node: '/root/Game/Player', props: ['position'], at: 0 },
    { node: '/root/Game/Player', props: ['position'], every: 1 },
  ]);
  assert.deepEqual(args.inputs, [{ key: 'W', at: 31, to: 120 }]);
  assert.deepEqual(args.screenshots, [30, 120]);
  assert.equal(args.frames, 120);
  assert.deepEqual([args.width, args.height], [640, 360]);
});

// --- traces -----------------------------------------------------------------

test('a watch splits at its last colon', () => {
  assert.deepEqual(parseWatch('/root/Game/Player:position'), { path: '/root/Game/Player', prop: 'position', id: '/root/Game/Player:position' });
  assert.throws(() => parseWatch('/root/Game/Player'), /bad --watch/);
});

test('numbers come out of both engines\' value spellings', () => {
  assert.deepEqual(numbersOf('Vector3(1, -2.5, 3e-2)'), [1, -2.5, 0.03]);
  assert.deepEqual(numbersOf('(1.0, 2.0)'), [1, 2]);
  assert.deepEqual(numbersOf('true'), [1]);
  assert.deepEqual(numbersOf([1, 2]), [1, 2]);
  assert.deepEqual(numbersOf(7), [7]);
  assert.equal(numbersOf('"hello"'), null);
});

test('MCP probe lines parse into a frame-sorted trace, keeping unresolved probes as errors', () => {
  const text = [
    'play: 3 frame(s) (f0→f3, 0.05s game time) in 12 ms · 0 print(s), 0 error(s)',
    'probes:',
    '  f0 /root/Game/Player: position=Vector3(0, 1, 2)',
    '  f2 /root/Game/Player: position=Vector3(0.5, 1, 2)',
    '  f1 /root/Game/Player: position=Vector3(0.25, 1, 2)',
    '  f1 /root/Game/Coin: no node at `/root/Game/Coin`',
    'game stopped',
  ].join('\n');
  const watches = [parseWatch('/root/Game/Player:position'), parseWatch('/root/Game/Coin:position')];
  const tr = parseMcpProbes(text, watches);
  assert.deepEqual(tr['/root/Game/Player:position'].map((s) => [s.frame, s.value]), [[0, [0, 1, 2]], [1, [0.25, 1, 2]], [2, [0.5, 1, 2]]]);
  assert.equal(tr['/root/Game/Coin:position'].length, 1);
  assert.match(tr['/root/Game/Coin:position'][0].error, /no node/);
});

test('two watched properties of the SAME node each keep their own values', () => {
  // The editor prints one probe line per property. Each watch used to record
  // the OTHER property's line as an error, and the per-frame dedupe then kept
  // whichever came last, so `--watch P:position --watch P:velocity` turned
  // position into all nulls (pat-d4ghb).
  const text = [
    'probes:',
    '  f0 /root/Game/Player: position=Vector3(0, 1, 2)',
    '  f0 /root/Game/Player: velocity=Vector3(0, 0, 0)',
    '  f1 /root/Game/Player: position=Vector3(0.5, 1, 2)',
    '  f1 /root/Game/Player: velocity=Vector3(3, 0, 0)',
  ].join('\n');
  const watches = [parseWatch('/root/Game/Player:position'), parseWatch('/root/Game/Player:velocity')];
  const tr = parseMcpProbes(text, watches);
  assert.deepEqual(tr['/root/Game/Player:position'].map((s) => s.value), [[0, 1, 2], [0.5, 1, 2]]);
  assert.deepEqual(tr['/root/Game/Player:velocity'].map((s) => s.value), [[0, 0, 0], [3, 0, 0]]);
});

test('the Godot capture normalises to the same trace shape', () => {
  const cap = { frames: [{ frame: 0, values: { 'a:p': [1, 2] } }, { frame: 1, values: { 'a:p': null }, errors: { 'a:p': 'no node at a' } }] };
  const tr = traceFromGodot(cap, [parseWatch('a:p')]);
  assert.deepEqual(tr['a:p'][0].value, [1, 2]);
  assert.match(tr['a:p'][1].error, /no node/);
});

test('alignment pairs frames by number, with an offset, and lists the unpaired ones', () => {
  const a = [0, 1, 2, 3].map((frame) => ({ frame, value: [frame] }));
  const b = [1, 2, 3, 4, 5].map((frame) => ({ frame, value: [frame] }));
  const same = alignTraces(a, b);
  assert.deepEqual(same.pairs.map((p) => p.frame), [1, 2, 3]);
  assert.deepEqual(same.onlyA, [0]);
  assert.deepEqual(same.onlyB, [4, 5]);
  // Patina frame f + 1 == Godot frame f: offset 1 pairs a[f] with b[f+1].
  const shifted = alignTraces(a, b, 1);
  assert.deepEqual(shifted.pairs.map((p) => [p.a.frame, p.b.frame]), [[0, 1], [1, 2], [2, 3], [3, 4]]);
  assert.deepEqual(shifted.onlyB, [4]);
});

test('the trace diff finds the first divergence, the max error and the per-axis error', () => {
  const ref = [0, 1, 2, 3, 4].map((frame) => ({ frame, value: [frame, 0, 0] }));
  const act = ref.map((s) => ({ frame: s.frame, value: [s.value[0] + (s.frame >= 2 ? 0.3 * (s.frame - 1) : 0), s.frame === 4 ? 0.4 : 0, 0] }));
  const d = diffTrace(ref, act, { tolerance: 0.01 });
  assert.equal(d.compared, 5);
  assert.equal(d.firstDivergence, 2);
  // frame 4: dx = 0.9, dy = 0.4 -> sqrt(0.97)
  assert.equal(d.maxErrorFrame, 4);
  assert.ok(Math.abs(d.maxError - Math.sqrt(0.97)) < 1e-9, String(d.maxError));
  assert.ok(Math.abs(d.componentMaxError[0] - 0.9) < 1e-9);
  assert.ok(Math.abs(d.componentMaxError[1] - 0.4) < 1e-9);
  assert.equal(d.componentMaxError[2], 0);
  assert.equal(d.pass, false);
});

test('an identical trace passes, and a missing frame or an unresolved node does not pass silently', () => {
  const ref = [0, 1, 2].map((frame) => ({ frame, value: [frame] }));
  const same = diffTrace(ref, ref.map((s) => ({ ...s })), { tolerance: 0 });
  assert.equal(same.pass, true);
  assert.equal(same.maxError, 0);
  assert.equal(same.firstDivergence, null);
  const gap = diffTrace(ref, ref.slice(0, 2));
  assert.deepEqual(gap.missingInActual, [2]);
  const broken = diffTrace(ref, [{ frame: 0, value: [0] }, { frame: 1, value: null, error: 'no node' }]);
  assert.equal(broken.firstDivergence, 1);
  assert.equal(broken.maxError, Infinity);
  assert.equal(diffTrace(ref, []).pass, false, 'nothing compared is not a pass');
});

// --- images -----------------------------------------------------------------

test('the image diff counts pixels over the threshold and averages the RGB distance', () => {
  const a = makeImage(4, 1, [100, 100, 100, 255]);
  const b = makeImage(4, 1, [100, 100, 100, 255]);
  b.data.set([140, 100, 100, 255], 0); // +40 red: over 32
  b.data.set([100, 120, 100, 255], 4); // +20 green: under 32
  b.data.set([100, 100, 70, 0], 8); //    -30 blue, alpha ignored: under
  const d = imageDiff(a, b, { threshold: 32 });
  assert.equal(d.mismatch, 0.25);
  assert.ok(Math.abs(d.meanDelta - (40 + 20 + 30) / 4) < 1e-9);
  assert.deepEqual(d.meanSignedDelta, [10, 5, -7.5]);
  // Heat map: the 40-delta pixel is red-ish, the untouched one black.
  assert.deepEqual([...d.heat.data.slice(12, 16)], [0, 0, 0, 255]);
  assert.ok(d.heat.data[0] > 100 && d.heat.data[1] === 0);
  assert.equal(imageDiff(a, a).mismatch, 0);
  // Two channels 20 apart: the LARGEST channel delta (20) decides, not their sum (40).
  assert.equal(imageDiff(makeImage(1, 1, [100, 100, 100, 255]), makeImage(1, 1, [120, 120, 100, 255]), { threshold: 32 }).mismatch, 0);
});

test('images of different sizes are compared after resampling the second', () => {
  const a = makeImage(4, 4, [10, 20, 30, 255]);
  const b = makeImage(2, 2, [10, 20, 30, 255]);
  const d = imageDiff(a, b);
  assert.equal(d.resized, true);
  assert.equal(d.mismatch, 0);
  const half = makeImage(2, 1, [0, 0, 0, 255]);
  half.data.set([200, 200, 200, 255], 4);
  assert.deepEqual([...resizeImage(half, 1, 1).data], [100, 100, 100, 255]);
});

test('the heat colour runs black -> red -> yellow -> white', () => {
  assert.deepEqual(heatColour(0), [0, 0, 0]);
  assert.deepEqual(heatColour(255), [255, 255, 255]);
  assert.deepEqual(heatColour(128), [255, 129, 0]);
});

test('PNG round trip is lossless, and the decoder undoes every filter type', () => {
  const img = makeImage(3, 2);
  for (let i = 0; i < img.data.length; i++) img.data[i] = (i * 37) % 256;
  assert.deepEqual(decodePng(encodePng(img)), img);
  // Hand-filtered RGB scanlines: filter 1 (Sub), 2 (Up), 3 (Average), 4 (Paeth).
  const w = 2, h = 6;
  const pixels = [[10, 20, 30, 40, 50, 60], [15, 25, 35, 45, 55, 65], [1, 2, 3, 250, 251, 252], [9, 8, 7, 6, 5, 4],
    // A Paeth tie (pa == pc, a != c): left 10, up 40, up-left 30 must predict from the LEFT.
    [30, 30, 30, 40, 40, 40], [10, 10, 10, 77, 77, 77]];
  const raw = [];
  let prev = new Array(6).fill(0);
  [1, 2, 3, 4, 0, 4].forEach((f, y) => {
    const row = pixels[y];
    raw.push(f);
    for (let x = 0; x < 6; x++) {
      const a = x >= 3 ? row[x - 3] : 0, b = prev[x], c = x >= 3 ? prev[x - 3] : 0;
      let pred = 0;
      if (f === 0) pred = 0;
      else if (f === 1) pred = a;
      else if (f === 2) pred = b;
      else if (f === 3) pred = (a + b) >> 1;
      else {
        const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
        pred = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      raw.push((row[x] - pred + 256) & 0xff);
    }
    prev = row;
  });
  const png = encodePng(makeImage(w, h)); // borrow the container, swap the header and data
  const sig = png.subarray(0, 8);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    return Buffer.concat([len, Buffer.from(type), data, Buffer.alloc(4)]); // CRC unchecked by the decoder
  };
  const rgb = Buffer.concat([sig, chunk('IHDR', ihdr), chunk('IDAT', zlib.deflateSync(Buffer.from(raw))), chunk('IEND', Buffer.alloc(0))]);
  const out = decodePng(rgb);
  const got = [];
  for (let p = 0; p < w * h; p++) got.push(...out.data.slice(p * 4, p * 4 + 3));
  assert.deepEqual(got, pixels.flat());
  assert.equal(out.data[3], 255, 'RGB decodes opaque');
});

test('side by side lays images left to right with a gap', () => {
  const s = sideBySide([makeImage(2, 1, [255, 0, 0, 255]), makeImage(1, 2, [0, 255, 0, 255])], 1, [9, 9, 9, 255]);
  assert.equal(s.width, 4);
  assert.equal(s.height, 2);
  assert.deepEqual([...s.data.slice(0, 4)], [255, 0, 0, 255]);
  assert.deepEqual([...s.data.slice(8, 12)], [9, 9, 9, 255]);
  assert.deepEqual([...s.data.slice(12, 16)], [0, 255, 0, 255]);
  assert.deepEqual([...s.data.slice(16, 20)], [9, 9, 9, 255], 'below the short image is background');
});

test('the verdict applies only the thresholds given', () => {
  const traces = { 'a:p': { compared: 10, maxError: 0.5, maxErrorFrame: 7, firstDivergence: 3 } };
  const images = [{ frame: 30, mismatch: 0.2, meanDelta: 12 }];
  assert.equal(verdict({ traces, images }).pass, true);
  const tight = verdict({ traces, images }, { maxTraceError: 0.1, maxMismatch: 10, maxMeanDelta: 20 });
  assert.equal(tight.pass, false);
  assert.equal(tight.failures.length, 2);
  assert.match(tight.failures[0], /max error 0.500 at f7 > 0.1 \(first divergence f3\)/);
  assert.match(tight.failures[1], /f30: 20.000% pixels differ > 10%/);
  assert.equal(verdict({ traces: { x: { compared: 0 } } }).pass, false, 'no common frames always fails');
});

test('the screenshot frames come from the play report header line', () => {
  assert.deepEqual(screenshotFrames('probes:\n  f1 x: y=1\nscreenshots: f30, f120, f198 (1152x648), in order below\n'), [30, 120, 198]);
  assert.deepEqual(screenshotFrames('no pictures'), []);
});

// --- project patching -------------------------------------------------------

const PROJECT = `config_version=5

[application]

config/name="Platformer 3D"
run/main_scene="res://game.tscn"
config/features=PackedStringArray("4.7", "Forward Plus")

[display]

window/size/viewport_width=800
`;

test('the capture autoload is registered first, and a newer feature version is lowered', () => {
  const { text, notes } = patchProjectGodot(PROJECT, { godotVersion: [4, 6] });
  assert.match(text, /\[autoload\]\n\nGodotReferenceCapture="\*res:\/\/__godot_reference_capture\.gd"\n/);
  assert.match(text, /config\/features=PackedStringArray\("4.6", "Forward Plus"\)/);
  assert.equal(notes.length, 1);
  assert.match(notes[0], /4\.7/);
  const withAutoload = PROJECT + '\n[autoload]\n\nMusic="*res://music.gd"\n';
  const patched = patchProjectGodot(withAutoload, { godotVersion: [4, 7] }).text;
  assert.ok(patched.indexOf('GodotReferenceCapture=') < patched.indexOf('Music='), 'before the game\'s own autoloads');
  assert.equal((patched.match(/\[autoload\]/g) || []).length, 1, 'one [autoload] section');
  assert.match(patched, /PackedStringArray\("4.7", "Forward Plus"\)/, 'same version is left alone');
});

test('project settings and Godot versions are read the way the capture needs them', () => {
  assert.equal(projectSetting(PROJECT, 'display/window/size/viewport_width'), '800');
  assert.equal(projectSetting(PROJECT, 'display/window/size/viewport_height'), null);
  assert.equal(projectSetting(PROJECT, 'application/run/main_scene'), '"res://game.tscn"');
  assert.deepEqual(parseGodotVersion('4.6.1.stable.official.14d19694e'), [4, 6]);
  assert.equal(resPath('level/level.tscn'), 'res://level/level.tscn');
  assert.equal(resPath('res://main.tscn'), 'res://main.tscn');
});

// --- random numbers (pat-y2ylj) ---------------------------------------------

test('clock-seeded randomness in a script is rewritten to the pinned helpers', () => {
  const src = [
    'var rng := RandomNumberGenerator.new()',
    'func _ready():',
    '\trandomize()',
    '\trng.randomize()',
    '\tvar r = RandomNumberGenerator.new( )',
    '\tmy_randomize()',
    '\tvar x = randf()',
  ].join('\n');
  const { text, count } = pinRandomnessInScript(src);
  assert.equal(count, 4);
  const pin = 'preload("res://__godot_reference_capture.gd")';
  assert.ok(text.startsWith(`var rng := ${pin}.pinned_rng()\n`), 'RandomNumberGenerator.new() takes the next instance seed');
  assert.ok(text.includes(`\tvar r = ${pin}.pinned_rng()`));
  assert.ok(text.includes(`\t${pin}.pinned_randomize()`), 'a bare randomize() re-seeds the global RNG from the pinned base');
  assert.ok(text.includes(`\trng.set_seed(${pin}.next_instance_seed())`), 'an instance randomize() takes the next instance seed');
  assert.ok(text.includes('\tmy_randomize()'), 'a function merely ending in randomize is left alone');
  assert.ok(text.includes('\tvar x = randf()'), 'draws are left alone: only the SEEDING is pinned');
  assert.equal(pinRandomnessInScript('func f():\n\tpass\n').count, 0);
});

test('the scenario carries a seed: --seed, else playtest.json, else the MCP play default', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'seed-scenario-'));
  try {
    fs.writeFileSync(path.join(dir, 'project.godot'), PROJECT);
    assert.equal(resolveScenario(dir, {}).seed, DEFAULT_SEED);
    assert.equal(DEFAULT_SEED, 42, 'the agent play tool pins 42 when no seed is given');
    fs.writeFileSync(path.join(dir, 'playtest.json'), JSON.stringify({ seed: 7 }));
    assert.equal(resolveScenario(dir, {}).seed, 7);
    assert.equal(resolveScenario(dir, { seed: 9 }).seed, 9);
    const t = parseTimeline('wait:100', 60);
    assert.equal(mcpPlayArgs({ frames: 6, timeline: t, watches: [], shots: [], size: [8, 8], seed: 9 }).seed, 9, 'Patina plays with the reference\'s seed');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test('the capture window is made one that can never take the focus', () => {
  const plain = withUnfocusableWindow('config_version=5\n\n[application]\n\nconfig/name="x"\n');
  assert.equal(projectSetting(plain, 'display/window/size/no_focus'), 'true');
  const display = withUnfocusableWindow('[display]\n\nwindow/size/viewport_width=480\nwindow/size/no_focus=false\n');
  assert.equal(projectSetting(display, 'display/window/size/no_focus'), 'true');
  assert.equal(projectSetting(display, 'display/window/size/viewport_width'), '480', 'the rest of [display] is kept');
  assert.equal((display.match(/no_focus/g) || []).length, 1, 'a game\'s own no_focus line is replaced, not duplicated');
  assert.equal((display.match(/^\[display\]/gm) || []).length, 1);
});
