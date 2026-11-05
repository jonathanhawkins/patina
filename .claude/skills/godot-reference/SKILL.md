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
- In Godot, the capture node runs first in the first physics tick of each
  iteration. It records frame f-1, takes the picture of the last drawn frame,
  then presses the inputs with `Input.parse_input_event` and
  `flush_buffered_events`. That is where a real key press lands, so
  `is_action_just_pressed` fires in the same frame.
- Godot runs with `--fixed-fps 60 --disable-vsync`. Every run is
  deterministic: two runs give byte-identical PNGs.

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
