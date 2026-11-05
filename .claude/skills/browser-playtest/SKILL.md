---
name: browser-playtest
description: Play a game in the Patina web editor the way a person does (a real headless Chrome) and report what the player gets — frames shown per second, page errors, refused requests, whether the game fills the view — plus the editor's own `diagnose` findings and, optionally, a profile of the hottest functions. Use after touching anything on the Play path (render loop, frame stream, input, editor JS), when someone says the game is laggy / unresponsive / looks wrong in the browser, and before claiming a game "works in the browser". Trigger on: "playtest", "laggy", "low fps", "frame rate", "can't play it in the browser", "does it work in the editor", "game looks wrong in the browser", "controls don't respond".
argument-hint: [project-dir] [--keys "KeyD:1500,Space:100"]
---

# Browser Playtest

The agent interface (`/mcp`) plays a game **headless and frame-exact** on the
game thread. That proves the game's logic, but it never goes through the path
a person uses: the browser page pulling frames, the stream, the input routes,
the panel-sized render. The person's experience can be broken while every
`play` report is clean. This skill measures the person's path.

What it catches, from real incidents:
- **Lag**: the Game view fetched a frame on a 200 ms timer, so a 60 fps game
  showed at 5 fps (shown fps vs server render fps; `diagnose` says
  stream-bound).
- **The debug-build trap**: a debug editor renders Play at ~30 ms per frame,
  about 10x slower than release (`build: debug` plus a finding).
- **Refused requests**: frames or keys refused with 429 look like a frozen
  view or a dropped key.
- **Dead controls**: a key whose keyup the OS swallowed (⌘⌃⇧4) left the game
  on a fallback input map (`--keys "down:MetaLeft,KeyD:1500"`).
- **Wrong framing**: the game drawn 1:1 in a corner because Play ignored
  project.godot's stretch mode (content share, and `diagnose`'s
  `stretch ignored`).
- **Invisible UI**: the FPS counter attached to a hidden container.
- **Bad audio**: `--audio 8` measures what the sound device played (underruns,
  clicks, gaps, clipping) and saves `audio.wav`; see the `audio-debug` skill.
- **Editor chrome in the game**: 3D Play showed the editor viewport (grid,
  origin axes, gizmos) instead of the game camera. The report flags the
  editor's origin axis in the Play frame as a problem (`editor overlay in the
  Play view`).
- **A player that never instanced**: `Player` came out as an empty `Node`, so
  its position never moved. With `--expect-move` that is a problem, and the
  watch line says what the node IS (`not on /root/Game/Player, a Node`).

## Steps

### 1. Start the editor in the background (release, for anything about speed)

```bash
scripts/editor-bg.sh start --release --build --port 9400 <project_dir> [scenes/main.tscn]
scripts/editor-bg.sh status --port 9400     # pid, build, live rates
```

`--build` compiles through `rust_task.sh` (~20 s incremental, ~2.5 min cold).
The first launch of a freshly built binary can sit at macOS's code-signing
check for a minute; the script waits up to 3 minutes. Never pipe a backgrounded
editor through `| head`: use this script, which redirects the whole subshell.

If the person already has the editor open, reuse that port; do not restart
an editor they are using without saying so.

### 2. Play

```bash
scripts/browser-playtest.mjs --url http://localhost:9400/editor \
  --keys "Space:120,wait:500,KeyD:1500,ArrowUp:800" --profile 3
```

- `--keys`: `Code:ms` holds with real auto-repeat, `down:`/`up:` press or
  release without the other half, `wait:ms`. The default is `wait:3000`, which
  measures without touching the game. Read the game's input map first
  (`scripts/patina-mcp status` lists the actions; project.godot holds the
  bindings) so the keys you send do something.
- `--profile N` samples the editor process for N seconds (macOS `sample`) and
  lists the hottest non-idle functions. Use it when `render_ms` is high.
  ⚠️ `sample` slows the editor (56 → 31 frames/s on the platformer), so a
  profiled run's frame rate is only a warning; judge fps on a plain run.
- Every DevTools call has a 30 s deadline and the whole run a budget (2 min +
  the steps + 4× the profile); on expiry it stops Play and exits 2, so a run
  can no longer sit silent for hours.
- `--edit-steps "select:TileMapLayer,wait:1500"` acts in the EDIT view before
  Play (expanding the scene tree to reach the row). Then every image the page
  references is fetched; one that does not load is a problem. That is how a
  404 on the tile palette's thumbnails shows up.
- `--soak 180` keeps playing for that long after the steps and samples memory
  and render time every 10 s. Growth above 10 MB/min, or render time up by
  30% and 3 ms, is a problem. It caught two leaks the 5 s runs never could.
- `--watch /root/Main/Player:position` reads a property after every step.
  A value that never changes is a warning. Add `--expect-move` when the keys
  should move it: then it is a problem. The gate adds it for any scenario with
  `expect` rules.
- The Play frame is checked for the editor's origin axes: a long, thin,
  OBLIQUE red or blue line holding most of that colour's thin pixels
  (`report.page.editor_axes`, `scripts/lib/playtest-checks.mjs`). Axis-aligned
  lines are skipped, because 2D borders and tile rows are axis-aligned. It was
  checked on all four 2D games with no false positives. It cannot see an axis
  that is off-screen or exactly horizontal or vertical, so treat a clean
  result as "no axis seen", not "no overlay".
- A 4xx from the editor's own `/api/` is a problem, not a warning.
- The report records the editor's age and memory and the host load. It warns
  when the editor is older than 1 h, holds more than 1.5 GB, or the load
  exceeds the CPU count.
- Exit status 0 = no problems, 1 = problems, 2 = could not run.

### The gate: every real game, every time

`node scripts/playtest-gate.mjs` runs each `games/<game>/playtest.json`
scenario on a fresh release editor and judges it: `expect` rules (a watched
value must increase, decrease or change across a step), errors, broken
images, fps, `game-bound`/`render-bound`, audio underruns, and render time
against `scripts/playtest-baselines.json`. Busy-host perf failures are re-run
once, then reported INCONCLUSIVE (exit 3). `--file-beads` files one `br` bug
per failing game; `--soak 180` adds the soak. Results go to
`.playtest-gate/latest.json`, which the planner reads. Add a `playtest.json`
to every new game in `games/` (see the `real-game-import` skill). The gate
validates each scenario first. The scene must exist, every step must parse,
and every `expect` rule must name a step in `keys` and a value in `watch`.
An invalid scenario is that game's ERROR, not a crash. Never run the gate on
someone else's ports: it uses `--port-base` + game index, so pass
`--port-base <yours> --games <one>` to stay on one port.

- It launches its own headless Chrome (throwaway profile, free port) and kills
  only that one. The person's Chrome is never touched, and no extension is
  needed, which matters when Claude in Chrome isn't connected.

### 3. Read the report

| Line | Healthy | If not |
|---|---|---|
| `shown: N frames/s` | ≥ 55 release | compare it with `server: render` |
| `server: game G/s · render R/s at M ms` | G = R = 60, M < 8 | G low: game-bound; R < G: render-bound |
| `counter:` | visible | the UI didn't attach it to the visible surface |
| `view: … game content … (P%)` | ~100% when the project stretches | low P: stretch not applied, or the game draws small |
| `editor diagnose` → `findings` | none | each finding names its bottleneck |
| `hottest functions` | spread out | one raster function dominating means optimise it, or build release |

- Shown much lower than rendered means the stream (page fetch cadence, 429s,
  decode).
- Rendered lower than game frames means the render.
- Game frames below 60 means scripts or physics, or the render holding the
  editor lock.

### 4. Dig in with the agent interface

```bash
PATINA_PORT=9400 scripts/patina-mcp diagnose                         # measured pipeline + findings
PATINA_PORT=9400 scripts/patina-mcp screenshot '{"view":"player"}'   # the exact frame the browser got
PATINA_PORT=9400 scripts/patina-mcp screenshot                       # the game rendered at its window size
```

Comparing the two screenshots is the fastest check for "looks right to the
agent, wrong to the person".

### 5. Fix, then re-run the same command

Report before and after numbers from the same `--keys` script and build. A fix
to the Play path is not done until the playtest is clean on a release build.
Reload any browser tab the person has open so it picks up new editor JS.

## Gotchas

- The `diagnose` call goes through the rate limiter like any other client. Run
  it a second after a request burst, or it is refused too.
- The content-share check treats Godot's default clear grey (76,76,76) as
  "not drawn". A game that paints that exact grey reads as smaller than it is.
- Screenshots and `report.json` go to a temp directory; the last line of the
  report prints its path.
