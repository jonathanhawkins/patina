#!/usr/bin/env node
// godot-reference: capture what REAL Godot does with a game, as ground truth
// for Patina (pat-xggzs).
//
//   node scripts/godot-reference.mjs <game_dir> [--scene X] [--keys "..."]
//        [--watch /root/Path:prop ...] [--frames N] [--size WxH] [--out dir]
//
// Copies the game to a temp dir (games/ is never written), injects a capture
// autoload (scripts/lib/godot-reference-capture.gd) that replays the --keys
// timeline at fixed physics frames, records every watched property every
// frame and saves PNG frames at the step boundaries, then runs Godot.app with
// real rendering (a window placed off-screen; `--headless` renders nothing).
//
// Output (--out, default $TMPDIR/godot-reference/<game>):
//   trace.json       {meta, frames: [{frame, values: {"/root/..:prop": [..]}}]}
//   frames/fNNNNN.png the rendered frame after N physics steps
//   godot.log         Godot's import + run output
//
// Defaults come from <game_dir>/playtest.json (scene, keys, watch). Frame
// numbering matches the Patina MCP `play` tool: see scripts/lib/godot-reference-capture.gd.
//
// Random numbers (pat-y2ylj): Godot seeds its RNG from the clock at startup,
// in `randomize()` and in `RandomNumberGenerator.new()`, so a game using
// randomness recorded a different reference each run. The capture seeds the
// global RNG with --seed before anything else runs, and the temp copy's
// scripts are rewritten so `randomize()` / `RandomNumberGenerator.new()` /
// `rng.randomize()` take seeds derived from it — the rule Patina's pinned
// `play` applies, with a generator that is a bit-exact port of Godot's.
//
// Options:
//   --scene X            scene to run (res:// path or relative), default run/main_scene (what Play runs)
//   --keys "..."          browser-playtest key steps: wait:ms, Code:ms (held), down:Code, up:Code
//   --watch path:prop     property to record every frame (repeatable)
//   --frames N            frames to run (default: the timeline's length; longer adds idle frames)
//   --size WxH            window size (default: the project's viewport size, else 1152x648)
//   --shots 30,90,...     frames to picture (default: the step boundaries, at most 8)
//   --fps N               frames per second the ms durations are translated at (default 60)
//   --rendering-method M  forward_plus (default, the project's) | mobile | gl_compatibility
//   --attempts N          runs of Godot before a disturbed capture is given up (default 3)
//   --seed N              random seed the run is pinned to (default: playtest.json `seed`, else 42 —
//                         the MCP `play` default). See "Random numbers" below.
//   --godot PATH          Godot binary (default $GODOT or /Applications/Godot.app/Contents/MacOS/Godot)
//   --keep-temp           keep the temp project copy (its path is printed)
//   --timeout S           kill Godot after S seconds (default 120 + frames/30)
//   --json                print the result summary as JSON

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parseTimeline, parseWatch, pickShotFrames } from './lib/godot-reference-lib.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const ROOT = path.resolve(HERE, '..');
const CAPTURE_GD = path.join(HERE, 'lib', 'godot-reference-capture.gd');
const AUTOLOAD_NAME = 'GodotReferenceCapture';
const AUTOLOAD_FILE = '__godot_reference_capture.gd';
/** The seed a run is pinned to when nothing names one: the MCP `play` default. */
export const DEFAULT_SEED = 42;
const PIN = `preload("res://${AUTOLOAD_FILE}")`;

/**
 * Rewrites one GDScript source so its clock-seeded randomness is pinned to the
 * capture's seed (see scripts/lib/godot-reference-capture.gd):
 * `rng.randomize()` -> `rng.set_seed(<next instance seed>)`, a bare
 * `randomize()` -> `pinned_randomize()`, `RandomNumberGenerator.new()` ->
 * `pinned_rng()`. Returns {text, count}.
 */
export function pinRandomnessInScript(text) {
  let count = 0;
  const bump = (f) => (...a) => { count++; return f(...a); };
  let out = String(text);
  out = out.replace(/\bRandomNumberGenerator\.new\(\s*\)/g, bump(() => `${PIN}.pinned_rng()`));
  out = out.replace(/\.randomize\(\s*\)/g, bump(() => `.set_seed(${PIN}.next_instance_seed())`));
  out = out.replace(/(^|[^\w.])randomize\(\s*\)/gm, bump((_, pre) => `${pre}${PIN}.pinned_randomize()`));
  return { text: out, count };
}

/** Applies {@link pinRandomnessInScript} to every .gd file under `dir`; returns the notes. */
export function pinRandomnessInProject(dir) {
  const notes = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) {
        if (e.name !== '.godot') walk(p);
      } else if (e.name.endsWith('.gd') && e.name !== AUTOLOAD_FILE) {
        const src = fs.readFileSync(p, 'utf8');
        const { text, count } = pinRandomnessInScript(src);
        if (count) {
          fs.writeFileSync(p, text);
          notes.push(`${path.relative(dir, p)}: ${count} clock-seeded RNG call(s) pinned to the run's seed`);
        }
      }
    }
  };
  walk(dir);
  return notes;
}

export function defaultGodot() {
  return process.env.GODOT || '/Applications/Godot.app/Contents/MacOS/Godot';
}

/** The `major.minor` Godot reports (`4.6.1.stable.official...` -> [4, 6]). */
export function parseGodotVersion(text) {
  const m = /(\d+)\.(\d+)/.exec(String(text));
  return m ? [Number(m[1]), Number(m[2])] : null;
}

/**
 * Rewrites a project.godot for the reference run:
 * - registers the capture autoload FIRST in [autoload] (so it exists before
 *   the game's own autoloads and the main scene);
 * - when `config/features` names a Godot newer than the one running (the 3D
 *   platformer declares "4.7" on 4.6.1), lowers it to the running version, so
 *   the editor import does not stop at the "made with a newer version" prompt.
 * `autoloadName`/`autoloadFile` name another capture autoload (scene-audit's).
 * Returns {text, notes}.
 */
export function patchProjectGodot(text, { godotVersion = null, autoloadName = AUTOLOAD_NAME, autoloadFile = AUTOLOAD_FILE } = {}) {
  const notes = [];
  let out = text.replace(/\r\n/g, '\n');
  const line = `${autoloadName}="*res://${autoloadFile}"`;
  if (/^\[autoload\]\s*$/m.test(out)) {
    out = out.replace(/^\[autoload\]\s*\n(\n?)/m, `[autoload]\n\n${line}\n`);
  } else {
    out = out.replace(/\n*$/, '\n') + `\n[autoload]\n\n${line}\n`;
  }
  if (godotVersion) {
    out = out.replace(/^(config\/features=PackedStringArray\()([^)]*)\)/m, (all, head, list) => {
      const items = list.split(',').map((s) => s.trim());
      const fixed = items.map((item) => {
        const m = /^"(\d+)\.(\d+)"$/.exec(item);
        if (!m) return item;
        const v = [Number(m[1]), Number(m[2])];
        if (v[0] > godotVersion[0] || (v[0] === godotVersion[0] && v[1] > godotVersion[1])) {
          notes.push(`project declares Godot ${v.join('.')} features; running ${godotVersion.join('.')}, so config/features was lowered to "${godotVersion.join('.')}" in the temp copy`);
          return `"${godotVersion.join('.')}"`;
        }
        return item;
      });
      return `${head}${fixed.join(', ')})`;
    });
  }
  return { text: out, notes };
}

/**
 * Makes the capture window one that can never become the KEY window
 * (`display/window/size/no_focus`, WINDOW_FLAG_NO_FOCUS: macOS
 * `canBecomeKeyWindow` answers NO). macOS Godot releases every held key when
 * its window RESIGNS key status (`windowDidResignKey` ->
 * `Input::release_pressed_events`), and a window launched from a script
 * gained and lost the key status at random in its first frames — Dodge the
 * Creeps' Enter (held f1..f18) was released at f1 or f2 in 2 of 4 runs — and
 * again whenever another Godot window opened. A window that is never key
 * never resigns. Input is unaffected: the capture parses its events straight
 * into `Input`, which does not look at focus. Returns the patched text.
 */
export function withUnfocusableWindow(text) {
  let out = text.replace(/^window\/size\/no_focus=.*\n?/m, '');
  const line = 'window/size/no_focus=true';
  if (/^\[display\]\s*$/m.test(out)) out = out.replace(/^\[display\]\s*\n/m, `[display]\n\n${line}\n`);
  else out = out.replace(/\n*$/, '\n') + `\n[display]\n\n${line}\n`;
  return out;
}

/** Reads a project setting's raw value from project.godot text (`section/key`). */
export function projectSetting(text, key) {
  const [section, ...rest] = key.split('/');
  const name = rest.join('/');
  let inSection = false;
  for (const line of text.split('\n')) {
    const s = /^\[(.+)\]\s*$/.exec(line);
    if (s) {
      inSection = s[1] === section;
      continue;
    }
    if (inSection && line.startsWith(`${name}=`)) return line.slice(name.length + 1).trim();
  }
  return null;
}

/** Scene argument normalised to res:// form. */
export function resPath(scene) {
  if (!scene) return null;
  return scene.startsWith('res://') ? scene : `res://${scene.replace(/^\.?\//, '')}`;
}

function readJson(p) {
  try {
    return JSON.parse(fs.readFileSync(p, 'utf8'));
  } catch {
    return null;
  }
}

/** Resolves the scenario from flags + the game's playtest.json + project.godot. */
export function resolveScenario(gameDir, opts) {
  const project = fs.readFileSync(path.join(gameDir, 'project.godot'), 'utf8');
  const playtest = readJson(path.join(gameDir, 'playtest.json')) || {};
  const fps = opts.fps || 60;
  const keys = opts.keys ?? playtest.keys ?? 'wait:1000';
  const watchSpecs = opts.watch && opts.watch.length ? opts.watch : playtest.watch || [];
  const watches = watchSpecs.map(parseWatch);
  const timeline = parseTimeline(keys, fps);
  const frames = Math.max(opts.frames || 0, timeline.frames, 1);
  const mainScene = (projectSetting(project, 'application/run/main_scene') || '').replace(/^"|"$/g, '');
  // The MAIN scene, as Play (F5) runs it: playtest.json's `scene` is the one the
  // editor opens for editing, and its watch paths (/root/Game/...) name the
  // main scene's tree.
  const scene = resPath(opts.scene || mainScene || playtest.scene);
  let size = opts.size;
  if (!size) {
    const w = Number(projectSetting(project, 'display/window/size/viewport_width')) || 1152;
    const h = Number(projectSetting(project, 'display/window/size/viewport_height')) || 648;
    size = [w, h];
  }
  const shots = opts.shots && opts.shots.length ? opts.shots.filter((f) => f >= 1 && f <= frames) : pickShotFrames(timeline, frames, 8);
  const seed = Number.isFinite(opts.seed) ? opts.seed : Number.isFinite(playtest.seed) ? playtest.seed : DEFAULT_SEED;
  return { gameDir, project, scene, keys, fps, timeline, frames, watches, size, shots, seed };
}

function copyGame(src, dst) {
  fs.cpSync(src, dst, {
    recursive: true,
    filter: (p) => {
      const rel = path.relative(src, p);
      return !(rel === '.godot' || rel.startsWith(`.godot${path.sep}`));
    },
  });
}

/** How many times a disturbed capture is run again before giving up. */
export const DEFAULT_ATTEMPTS = 3;

/**
 * A capture that something outside the timeline disturbed on every attempt
 * (pat-y2ylj follow-up): a held key the OS released (window focus moved), or
 * an iteration Godot did not draw (window occluded).
 */
export class DisturbedCaptureError extends Error {
  constructor(game, attempts, disturbances) {
    const first = disturbances.slice(0, 4).map((d) => `${d.kind} at f${d.frame}${d.detail ? ` (${d.detail})` : ''}`).join(', ');
    super(
      `every one of ${attempts} Godot capture(s) of ${game} was disturbed — ${first}. ` +
        'A held key released by a focus change re-fires is_action_just_pressed, and an undrawn frame leaves a stale picture, ' +
        'so such a run is not a reference. Something keeps taking the capture window\'s focus or hiding it ' +
        '(another Godot window, a display sleep); run again when the desktop is quiet.',
    );
    this.disturbances = disturbances;
  }
}

/** Whether a pid is a live process. */
function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === 'EPERM';
  }
}

/**
 * Runs `fn` holding the machine-wide capture-window lock, so two captures (two
 * agents, two worktrees) never have windows open at once: a Godot window
 * opening takes the focus from the other one, and macOS Godot releases every
 * held key when its window resigns key status. A stale lock (dead pid) is
 * taken over; after `waitMs` the capture runs anyway (a disturbed run is
 * detected and retried regardless).
 */
export function withCaptureWindowLock(fn, { dir = path.join(os.tmpdir(), 'godot-reference-window.lock'), waitMs = 15 * 60_000 } = {}) {
  const deadline = Date.now() + waitMs;
  const nap = new Int32Array(new SharedArrayBuffer(4));
  let held = false;
  while (!held) {
    try {
      fs.mkdirSync(dir);
      fs.writeFileSync(path.join(dir, 'pid'), String(process.pid));
      held = true;
    } catch (e) {
      if (e.code !== 'EEXIST') throw e;
      let pid = NaN;
      try {
        pid = Number(fs.readFileSync(path.join(dir, 'pid'), 'utf8'));
      } catch {
        // being written: wait
      }
      if (Number.isFinite(pid) && pid > 0 && !alive(pid)) {
        fs.rmSync(dir, { recursive: true, force: true });
        continue;
      }
      if (Date.now() > deadline) break;
      Atomics.wait(nap, 0, 0, 250);
    }
  }
  try {
    return fn();
  } finally {
    if (held) fs.rmSync(dir, { recursive: true, force: true });
  }
}

/**
 * Runs the whole capture. Returns {outDir, trace, frames: [{frame, png}], log, notes}.
 * Throws with Godot's log tail when Godot fails.
 */
export function runGodotReference(gameDir, opts = {}) {
  gameDir = path.resolve(gameDir);
  const gamesDir = path.join(ROOT, 'games');
  const sc = resolveScenario(gameDir, opts);
  const outDir = path.resolve(opts.out || path.join(os.tmpdir(), 'godot-reference', path.basename(gameDir)));
  if (outDir === gamesDir || outDir.startsWith(gamesDir + path.sep)) throw new Error(`--out must not be inside games/ (${outDir})`);
  fs.rmSync(path.join(outDir, 'frames'), { recursive: true, force: true });
  fs.mkdirSync(path.join(outDir, 'frames'), { recursive: true });

  const godot = opts.godot || defaultGodot();
  if (!fs.existsSync(godot)) throw new Error(`Godot not found at ${godot} (set --godot or $GODOT)`);
  const versionText = spawnSync(godot, ['--version'], { encoding: 'utf8' }).stdout.trim();
  const godotVersion = parseGodotVersion(versionText);

  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), `godot-ref-${path.basename(gameDir)}-`));
  const log = [];
  try {
    copyGame(gameDir, tmp);
    const patched = patchProjectGodot(sc.project, { godotVersion });
    fs.writeFileSync(path.join(tmp, 'project.godot'), withUnfocusableWindow(patched.text));
    fs.copyFileSync(CAPTURE_GD, path.join(tmp, AUTOLOAD_FILE));
    const notes = [...patched.notes, ...pinRandomnessInProject(tmp)];

    const config = {
      out_dir: outDir,
      frames: sc.frames,
      inputs: sc.timeline.inputs.map(({ key, start, end }) => ({ key, start, end })),
      watches: sc.watches,
      shots: sc.shots,
      seed: sc.seed,
      // Test hook (tests/godot-reference-determinism): frames after which the
      // capture releases its held keys the way a focus loss does.
      simulate_focus_loss: opts.simulateFocusLossAt || [],
      simulate_undrawn: opts.simulateUndrawnAt || [],
    };
    const configPath = path.join(tmp, '__godot_reference_config.json');

    // First import: builds .godot/imported (textures, meshes, GridMap
    // libraries) so the run loads the same resources the editor would.
    const imp = spawnSync(godot, ['--headless', '--path', tmp, '--import'], { encoding: 'utf8', timeout: 300_000 });
    log.push('$ godot --headless --import', imp.stdout || '', imp.stderr || '');
    if (imp.status !== 0) notes.push(`import exited ${imp.status}${imp.signal ? ` (${imp.signal})` : ''}`);

    const args = [
      '--path', tmp,
      '--fixed-fps', String(sc.fps),
      '--disable-vsync',
      '--resolution', `${sc.size[0]}x${sc.size[1]}`,
      // A real window renders; one far off-screen keeps it out of the way.
      // (`--headless` uses the dummy renderer: every frame is blank.)
      '--position', '20000,20000',
    ];
    if (opts.renderingMethod) args.push('--rendering-method', opts.renderingMethod);
    args.push(sc.scene, '--', '--godot-reference-config', configPath);
    const timeout = (opts.timeout || 120 + Math.ceil(sc.frames / 30)) * 1000;
    const tracePath = path.join(outDir, 'trace.json');
    // 🔑 A run that something outside the timeline disturbed (a held key the
    // OS released, an undrawn frame: see the capture's header) is thrown away
    // and run again; only an undisturbed run is a reference.
    const attempts = Math.max(1, opts.attempts ?? DEFAULT_ATTEMPTS);
    let trace = null;
    const disturbed = [];
    for (let attempt = 1; attempt <= attempts; attempt++) {
      fs.rmSync(tracePath, { force: true });
      fs.rmSync(path.join(outDir, 'frames'), { recursive: true, force: true });
      fs.mkdirSync(path.join(outDir, 'frames'), { recursive: true });
      // The test hook disturbs the first attempt only, unless told otherwise.
      const first = attempt === 1 || opts.simulateEveryAttempt;
      fs.writeFileSync(configPath, JSON.stringify({
        ...config,
        simulate_focus_loss: first ? config.simulate_focus_loss : [],
        simulate_undrawn: first ? config.simulate_undrawn : [],
      }, null, 2));
      const run = withCaptureWindowLock(() => spawnSync(godot, args, { encoding: 'utf8', timeout }));
      log.push(`$ godot ${args.join(' ')}   # attempt ${attempt}`, run.stdout || '', run.stderr || '');
      fs.writeFileSync(path.join(outDir, 'godot.log'), log.join('\n'));
      if (!fs.existsSync(tracePath)) {
        const tail = log.join('\n').split('\n').slice(-30).join('\n');
        throw new Error(`Godot wrote no trace (exit ${run.status}${run.signal ? `, ${run.signal}` : ''}):\n${tail}`);
      }
      const t = JSON.parse(fs.readFileSync(tracePath, 'utf8'));
      const d = t.disturbances || [];
      // `acceptDisturbed` keeps a disturbed run anyway: for diagnosing one.
      if (d.length === 0 || opts.acceptDisturbed) {
        trace = t;
        if (disturbed.length) notes.push(`${disturbed.length} disturbed attempt(s) discarded before this capture: ${disturbed.map((x) => `${x.kind} at f${x.frame}`).slice(0, 4).join(', ')}`);
        trace.attempts = attempt;
        break;
      }
      disturbed.push(...d);
    }
    if (!trace) throw new DisturbedCaptureError(path.relative(ROOT, gameDir) || gameDir, attempts, disturbed);
    if (trace.quit_early) notes.push('the game left the tree before the timeline ended');
    if (trace.physics_ticks_per_second < sc.fps) {
      notes.push(`physics_ticks_per_second ${trace.physics_ticks_per_second} < ${sc.fps} fps: frames with no physics tick are not recorded`);
    }
    if (trace.physics_interpolation) {
      notes.push('physics interpolation is on: pictures may show nodes up to one physics tick behind their recorded property');
    }
    const errors = log.join('\n').split('\n').filter((l) => /^(SCRIPT )?ERROR:/.test(l.trim()));
    trace.meta = {
      tool: 'godot-reference',
      game: path.relative(ROOT, gameDir),
      scene: sc.scene,
      keys: sc.keys,
      fps: sc.fps,
      frames: sc.frames,
      size: sc.size,
      shots: sc.shots,
      seed: sc.seed,
      timeline: sc.timeline,
      watches: sc.watches,
      godot: versionText,
      notes,
      errors: errors.slice(0, 50),
    };
    fs.writeFileSync(tracePath, JSON.stringify(trace, null, 2));
    const frames = fs.readdirSync(path.join(outDir, 'frames'))
      .map((f) => /^f(\d+)\.png$/.exec(f))
      .filter(Boolean)
      .map((m) => ({ frame: Number(m[1]), png: path.join(outDir, 'frames', m[0]) }))
      .sort((a, b) => a.frame - b.frame);
    return { outDir, trace, frames, notes, tmp: opts.keepTemp ? tmp : null };
  } finally {
    if (!opts.keepTemp) fs.rmSync(tmp, { recursive: true, force: true });
  }
}

export function parseCommonArgs(argv, extra = {}) {
  const opts = { watch: [] };
  const rest = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => {
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      return argv[++i];
    };
    if (a === '--scene') opts.scene = value();
    else if (a === '--keys') opts.keys = value();
    else if (a === '--watch') opts.watch.push(value());
    else if (a === '--frames') opts.frames = Number(value());
    else if (a === '--fps') opts.fps = Number(value());
    else if (a === '--size') {
      const m = /^(\d+)x(\d+)$/.exec(value());
      if (!m) throw new Error('--size wants WxH');
      opts.size = [Number(m[1]), Number(m[2])];
    } else if (a === '--shots') opts.shots = value().split(',').map(Number).filter(Number.isFinite);
    else if (a === '--out') opts.out = value();
    else if (a === '--rendering-method') opts.renderingMethod = value();
    else if (a === '--godot') opts.godot = value();
    else if (a === '--keep-temp') opts.keepTemp = true;
    else if (a === '--timeout') opts.timeout = Number(value());
    else if (a === '--seed') opts.seed = Number(value());
    else if (a === '--attempts') opts.attempts = Number(value());
    else if (a === '--json') opts.json = true;
    else if (extra[a]) extra[a](opts, value);
    else if (a === '-h' || a === '--help') opts.help = true;
    else if (a.startsWith('--')) throw new Error(`unknown option ${a}`);
    else rest.push(a);
  }
  return { opts, rest };
}

function usage(file) {
  const src = fs.readFileSync(file, 'utf8').split('\n');
  return src.filter((l) => l.startsWith('//')).map((l) => l.replace(/^\/\/ ?/, '')).join('\n');
}

async function main() {
  let parsed;
  try {
    parsed = parseCommonArgs(process.argv.slice(2));
  } catch (e) {
    console.error(`godot-reference: ${e.message}`);
    process.exit(2);
  }
  const { opts, rest } = parsed;
  if (opts.help || rest.length !== 1) {
    console.log(usage(fileURLToPath(import.meta.url)));
    process.exit(opts.help ? 0 : 2);
  }
  const res = runGodotReference(rest[0], opts);
  const t = res.trace;
  if (opts.json) {
    console.log(JSON.stringify({ outDir: res.outDir, meta: t.meta, frames: res.frames }, null, 2));
    return;
  }
  console.log(`godot-reference: ${t.meta.game} ${t.meta.scene} — ${t.version} ${t.rendering_method}/${t.rendering_driver}, window ${t.window.join('x')}`);
  console.log(`  keys: ${t.meta.keys}  (${t.meta.frames} frames at ${t.meta.fps} fps)`);
  for (const i of t.meta.timeline.inputs) console.log(`    ${i.key.padEnd(6)} held f${i.start}..f${i.end}`);
  for (const n of t.meta.notes) console.log(`  note: ${n}`);
  if (t.meta.errors.length) console.log(`  ${t.meta.errors.length} error line(s) in godot.log, first: ${t.meta.errors[0]}`);
  const at = new Map(t.frames.map((f) => [f.frame, f]));
  for (const w of t.meta.watches) {
    console.log(`  ${w.id}:`);
    for (const f of new Set([0, ...t.meta.timeline.boundaries.map((b) => b.frame), t.meta.frames])) {
      const s = at.get(f);
      if (s) console.log(`    f${String(f).padEnd(5)} ${JSON.stringify(s.values[w.id])}`);
    }
  }
  console.log(`  frames: ${res.frames.map((f) => `f${f.frame}`).join(', ') || 'none'}`);
  console.log(`  out: ${res.outDir}`);
  if (res.tmp) console.log(`  temp project kept: ${res.tmp}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((e) => {
    console.error(`godot-reference: ${e.message}`);
    process.exit(1);
  });
}
