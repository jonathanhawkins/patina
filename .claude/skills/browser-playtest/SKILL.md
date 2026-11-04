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
- Exit status 0 = no problems, 1 = problems, 2 = could not run.
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
