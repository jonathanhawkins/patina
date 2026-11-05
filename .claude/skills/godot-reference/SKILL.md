---
name: godot-reference
description: Capture what REAL Godot (Godot.app 4.6.1) does with a game — the rendered frames and a per-frame trace of watched properties under a scripted key timeline — and diff Patina's Play against it, frame-exact. Reports the first diverging frame and max error per property, and per picture a mismatch %, a mean colour delta, a heat map and a Godot | Patina | heat side-by-side. Use for every visual or behaviour bug on a real game (games/*), before filing or closing such a bead, and whenever a fix is judged "looks right" against games/*/screenshots by eye. Trigger on: "godot reference", "compare to godot", "what does godot do", "ground truth", "diff against godot", "reference frame", "player moves differently", "looks different from godot".
argument-hint: <game_dir> [--keys "wait:500,KeyW:1500,Space:300"] [--watch /root/Game/Player:position]
---

# Godot reference

Every visual or behaviour bug on a real game used to be judged by eye against
`games/*/screenshots`. This gives ground truth: the same scenario, frame by
frame, in real Godot and in Patina.

- `scripts/godot-reference.mjs` runs Godot only. It copies the game to a temp
  dir (`games/` is never written), injects the capture autoload
  `scripts/lib/godot-reference-capture.gd`, imports, and runs Godot.app with
  real rendering.
- `scripts/compare-to-godot.mjs` runs the reference, then the same scenario in
  Patina through the editor's MCP `play` tool, and diffs the two.

## Run it

```bash
# Patina side needs your own editor on the game (release build):
scripts/editor-bg.sh start --release --build --port <P> games/platformer_3d game.tscn

node scripts/compare-to-godot.mjs games/platformer_3d --port <P> \
  --keys "wait:500,KeyW:1500,Space:300,wait:1000" \
  --watch /root/Game/Player:position --out .scratch/cmp-p3d
```

Defaults come from `games/<g>/playtest.json` (`keys`, `watch`) and
project.godot (`run/main_scene`, viewport size). The run takes ~10 s.

Godot alone, to get reference numbers or pictures without an editor:

```bash
node scripts/godot-reference.mjs games/platformer --out .scratch/ref-plat
```

Options shared by both: `--scene`, `--keys`, `--watch path:prop` (repeat it),
`--frames N`, `--size WxH`, `--shots 5,120`, `--fps`, `--rendering-method`,
`--godot PATH`, `--keep-temp`, `--json`. Compare-only: `--port`,
`--reference DIR` (reuse a capture), `--tolerance`, `--offset N`,
`--pixel-threshold`. Thresholds make it a gate: `--max-trace-error E`,
`--max-mismatch PCT`, `--max-mean-delta D`. It exits 1 when one fails, and 0
with no thresholds (report only).

## Read the output

```
trace /root/Game/Player:position: 199 frames compared, first divergence f8 (tolerance 0.01), max error 15.219 at f198 ...
  frame   godot                    patina                   error
  f0      [-9.498, -3.842, 3.933]  [-9.498, -3.842, 3.933]  1.73e-7
  f8      [-9.498, -3.999, 3.933]  [-9.498, -4.025, 3.933]  0.0263
picture f30: 86.7% pixels differ (coarse 85.3%), mean RGB delta 80.1, Patina-Godot per channel [39.7, 25.9, 47.5]
  .../diff/f00030-side.png
```

- The **first divergence** is where to look. Frames before it match Godot
  (above, f1 to f7 agree to 1e-6, so the alignment is exact). Here the player
  lands on the floor at y = -3.999 in Godot and falls through it in Patina.
- **Look at `diff/fNNNNN-side.png`** (Godot | Patina | heat map). The heat map
  runs black (same) to red to yellow to white (255 apart).
- `coarse` is the mismatch at 1/8 scale. A high full-scale figure with a low
  coarse one is texture noise or aliasing. When both are high, the layout,
  camera or colour differs.
- The per-channel signed delta says which way the colour is off (positive
  means Patina is brighter in that channel).
- Everything is in `report.json`, with both full series. Each side's raw data
  is in `godot/` and `patina/` (including `patina/play-report.txt`, which has
  the game's errors).

**Attach to the bead:** the trace table, the side-by-side path, and the
command.

## How the frames line up (do not "fix" this)

- Frame 0 is the state after every `_ready`, before anything steps. Frame f
  is the state after f main-loop iterations. At the 3D platformer's
  `physics_ticks_per_second=120`, each iteration is TWO physics ticks, so do
  not count physics ticks.
- An input `{start, end}` is pressed before frame `start` steps and released
  after frame `end`, which is the MCP `play` semantics. `--keys` ms become
  frames at `--fps` (60): `wait:500,KeyW:1500` holds W for f31 to f120.
- In Godot, the capture node records frame f at the very end of iteration f
  (a deferred call from its last-priority `_process`), then parses frame
  f+1's key events WITHOUT flushing them: the OS loop flushes between
  iterations, which is where a real key press lands, so
  `is_action_just_pressed` holds for the first physics tick of frame f+1 and
  only that one (Godot marks a press `physics_frames + 1`). Pictures are taken
  in the first physics tick of the next iteration (the last drawn frame).
  ⚠️ Inputs starting on frame 1 are still pressed inside the first tick, so
  for them alone `just_pressed` lands one tick late. (Before pat-dt40j every
  input was pressed that way and every `just_pressed` was one tick late — the
  2D Platformer's first jump frame moved one tick instead of two.)
- Godot runs with `--fixed-fps 60 --disable-vsync`. Every run is
  deterministic: two runs give byte-identical PNGs.
- Random numbers are PINNED (pat-y2ylj). Godot seeds its RNG from the clock
  at startup, in `randomize()` and in `RandomNumberGenerator.new()`, so a game
  using randomness (Dodge the Creeps' mobs) recorded a different reference
  every run. The capture calls `seed(--seed)` before any other autoload
  exists (default: playtest.json `seed`, else 42, the MCP `play` default),
  and the temp copy's scripts are rewritten so the k-th `randomize()` seeds
  `seed + k` and the j-th `RandomNumberGenerator.new()` / `rng.randomize()`
  gets `seed + 1000000 + j`. Patina's MCP `play` takes the same `seed` and
  pins the same three by the same rule (`gdcore::rng::pin_seed`), with a
  bit-exact port of Godot's PCG32, `randf`/`randf_range`/`randi_range`/
  `randfn`/`pick_random`/`shuffle`, and the draws Godot's constructors make
  (a GPUParticles2D takes two). Dodge the Creeps' mobs now match frame for
  frame. `PATINA_RNG_SEED=<n>` pins the editor's Play button the same way.

## Godot on this Mac: what works

- `--headless` uses the dummy renderer, so every picture is blank. The
  capture opens a real window at `--position 20000,20000` (off-screen) and
  reads `get_viewport().get_texture().get_image()`. Forward+ on Metal works.
  You do not need `--write-movie`.
- First import: `--headless --import` runs before the game, in the temp copy.
- Newer projects: platformer_3d declares `config/features "4.7"`. The temp
  copy lowers it to the running version (reported as a `note:`). A 4.7-only
  feature would then show up as an `ERROR:` line, counted in `trace.json`
  `meta.errors` and kept whole in `godot.log`.
- With physics interpolation on (both platformers), a picture can trail its
  property by up to one physics tick. The tool prints a note.

## The standing gate: parity can only improve

`node scripts/playtest-gate.mjs --godot-only --games <g> --port-base <P>`
(or `--godot` alongside the browser playtest) runs compare-to-godot for each
game with its playtest.json keys and watches, and scores it: per property the
first divergence frame and max error, per shot frame the mismatch % and mean
RGB delta. Each score is compared with `scripts/godot-parity-baselines.json`.
Worse beyond the noise margin (`DEFAULT_MARGINS` in
`scripts/lib/godot-parity.mjs`; a game may override them under `margins`) is a
functional failure. Better is reported as `improved`. A baseline recorded
under another scenario or Godot version reads `stale` and is not compared.

- Output: `.playtest-gate/parity.md` (`node scripts/parity-report.mjs`
  rebuilds it), the full compare in `.playtest-gate/parity/<g>/`.
- The Godot capture is cached in `.playtest-gate/godot-cache/<g>/<key>/`. The
  key hashes the game files (minus hidden `.godot/` and `.editor/`), the resolved scenario, `godot
  --version` and the capture tool, so a second run only plays Patina.
- On a cache miss Godot captures TWICE and the reference is cached only when
  both agree (every watched value; every picture within 2 per channel). An
  unstable reference is refused with the differing frames named — it would
  report regressions that are only Godot disagreeing with itself.
  `--godot-cache DIR` shares one cache between worktrees.
- 3D captures need the real off-screen window (above). Run the gate from a
  logged-in macOS session, never `--headless`.
- `--update-godot-baselines` is for a person or the coordinator, after
  merging. Agents never run it: re-recording hides the regression it would
  have caught. An improvement you made is re-recorded by the coordinator.
- A frame where NEITHER engine has the watched node (a title screen before
  the scene change) counts as a match; one side missing is infinite.
- Tests: `node --test scripts/tests/godot-parity.test.mjs`.

## Pitfalls

- Watch paths name the MAIN scene's tree (`/root/Game/...`). The tool runs
  `run/main_scene`, as Play does, not playtest.json's `scene` (that one is
  only the scene the editor opens). Pass `--scene` to run another. A watch
  that resolves to nothing shows `null` and `max error Infinity`.
- The Patina picture is the MCP `play` screenshot. For 3D it goes through
  `play_3d::prepare_play_3d_frame`, the Game tab's own entry point, so it
  matches what the browser shows (checked against browser-playtest's
  `end.png`). It does not show browser-stream problems (use
  `browser-playtest` for those).
- MCP `play` takes at most 8 screenshots per call. By default the shots are
  the step boundaries, thinned to 8.
- Godot cannot picture frame 0. Nothing has been drawn yet.
- Unit tests for the pure logic (timeline, alignment, metrics, PNG codec):
  `node --test scripts/tests/godot-reference.test.mjs`.
