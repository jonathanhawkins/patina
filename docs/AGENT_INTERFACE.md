# Patina Agent Interface (MCP)

The editor server speaks the Model Context Protocol at `POST /mcp`, on the same
port as the browser editor. An AI client and a human share one live session:
edits an agent makes appear in the open editor, and a game the human plays in
the browser leaves its output in the log the agent reads.

The goal is the best engine-to-AI interface there is. Patina owns the whole
stack, so the agent gets what bolt-on Unity/Unreal plugins cannot give it:
headless, frame-exact, faster-than-real-time play with scripted input, and
every node, property, print and error is observable.

## Quick start

```bash
cd my_project && cargo run --example editor -- --port 8080 main.tscn   # from engine-rs/
claude mcp add --transport http patina-editor http://127.0.0.1:8080/mcp  # native tools, next session
scripts/patina-mcp status                                               # or from any shell
scripts/patina-mcp play '{"frames":120,"inputs":[{"action":"jump","at":10}],"screenshots":[120]}'
```

`scripts/patina-mcp` prints text results and writes screenshots to
`$PATINA_SHOTS` (default `$TMPDIR/patina-shots`). Set `PATINA_PORT` and, if the
editor was started with a token, `PATINA_TOKEN`.

## Tools

| Tool | What it does |
|---|---|
| `status` | Project, open scene, game state, input actions, autoloads, files, last errors |
| `tree` | Scene (or `running: true` live game) outline: `Name (Class) [script] <instance> {groups}` + non-default props |
| `inspect` | One node: non-default props, script variables (live values), children |
| `edit` | Batch of ops — `add`, `set`, `delete`, `rename`, `move`, `instance`, `script`, `connect`, `group` — each through the editor's own undoable route |
| `scene` | `new` / `open` / `save` |
| `write_script` | Write a `.gd`, get syntax errors (line/col + caret) and static-check findings, optionally attach |
| `play` | Boot paused, step N frames against an input timeline, report frame-tagged prints/errors, probes, screenshots |
| `step` | Continue a paused game (`play` with `keep_running`) |
| `stop`, `screenshot`, `log` | Stop; render the game view (`view: "player"`: the exact frame the browser's Game view was last sent); read output since a sequence number |
| `diagnose` | Why Play feels slow or looks wrong in the browser, measured. Covers game frames stepped/s, frames rendered/s and ms each, and frames the browser actually received. Also reports 429s by route, the build profile, the running window's stretch against project.godot, and held keys. Ends with findings naming the bottleneck. |
| `class_info` | What Patina's ClassDB registers for a class |
| `api` | Any of the editor's HTTP routes |

## Checking what a person sees

`play` runs headless and frame-exact on the game thread. That proves the
logic, but not the browser path a person plays through: the page pulling
frames, the input routes, the panel-sized render. Three tools cover that path:

```bash
scripts/editor-bg.sh start --release --build --port 9400 games/spark_keeper scenes/title.tscn
scripts/browser-playtest.mjs --url http://localhost:9400/editor --keys "Space:120,KeyD:1500" --profile 3
PATINA_PORT=9400 scripts/patina-mcp diagnose
```

- `editor-bg.sh` starts, stops and checks a background editor. It never holds
  the caller's pipe open.
- `browser-playtest.mjs` plays in its own headless Chrome. It reports frames
  shown per second, page errors, 429s, the FPS counter, and how much of the
  view the game fills, followed by `diagnose`'s findings and optionally the
  hottest functions. Exit 1 on problems.
- The `browser-playtest` skill (`.claude/skills/browser-playtest/`) has the
  workflow and how to read the numbers. Judge frame rate on a release editor:
  a debug build renders Play about 10x slower.

## Conventions

- **Paths** are relative to the open scene's root (`Player/Sprite`; `.` is the
  root); `/root/...` is absolute.
- **Values** are Godot literals as strings — `"Vector2(10, 20)"`,
  `"Color(1, 0, 0)"` — or JSON numbers/bools. A string property keeps its text
  verbatim. `"res://x.png"` on a resource property loads it. A sub-resource
  is `{"new": "RectangleShape2D", "size": "Vector2(32, 32)"}`.
- **Frames** in `inputs`/`probes`/`screenshots` count from 1 within the call (0 =
  before stepping). Reports use absolute game frames (`f37`). An input is
  `{"action": "jump", "at": 10, "frames": 3}`, `{"key": "Space", "at": 5}` or
  `{"click": [x, y], "at": 20}`.
- Edits are refused while the game runs: `stop` first.

## Design rules

1. **One call is one experiment.** `play` returns everything a test needs in one round trip.
2. **Deterministic.** Play starts paused and steps a fixed timestep, so the same call gives the same report.
3. **Compact text, not JSON dumps.** Every byte returned is context the agent pays for.
4. **Same routes as the human.** Mutations go through the editor's own `/api` routes over loopback, so a bug the agent hits is a bug the browser has. Loopback calls carry a per-process key that exempts them from the browser's rate limit, and nothing else does.
5. **Errors are actionable.** Unknown node: the parent's children are listed. Unknown action: the declared actions are listed.

## Fixed while building it

These were reachable from the browser editor too:

- Play never drained the game's `print()` or runtime errors. They now reach the
  Output panel (`std`), the Errors tab, and the agent's frame-tagged log.
- A failed script call went to `tracing::debug!` only. The tree now keeps a capped
  error log (`SceneTree::drain_script_output`).
- Attaching a script by path (the Create Script dialog) never ran in Play and saved
  `script = "res://x.gd"`, which Godot rejects. It now writes `_script_path` plus
  `script = ExtResource(..)`.
- A "New RectangleShape2D" could not be sized and collided as nothing.
- A Control's `size`/`position` were undone by the next reflow, and the scene saved
  `size = Vector2(0, 0)`.
- `/api/script` and `/api/script/save` resolved `res://` against the process cwd
  instead of the opened project.
- `examples/editor.rs` attached demo scripts to any node named `Player`/`Enemy`
  in any scene.

Fixed while building `games/spark_keeper`, the first game made through this
interface:

- Play's game code ran on several threads: `_ready` on the HTTP thread that
  served Play, frames on the driver. The example's loop also stepped the game,
  so it ran about 1.6× too fast. Every script-made engine object (RNG, Image,
  JSON...) lives in a thread-local registry, so an object from `_ready` was gone
  a frame later. Game code now runs on the one runtime thread
  (`run_on_game_thread`), at a true 60 fps.
- Editor Play never set `res://`/`user://` roots, so `change_scene_to_file`,
  `load()`, FileAccess and ConfigFile failed. It never set a current scene, so a
  scene change left the old scene running. Autoloads came after the scene under
  `/root`, and an autoload whose script failed to compile got no script, silently.
- `change_scene_to_file` left instanced sub-scenes as childless placeholders.
- GDScript: a member initializer could not read a class `const`. `member.text = x`
  failed when the member held a node. `global_position` was unreadable in 2D
  scripts. A built-in signal could not be read as a value
  (`body_entered.connect`). A constant read through an instance was null.
  `is`/`as` failed for every node class. A script connecting its own signal from
  `_ready` was refused.
- Containers laid out against the viewport instead of their own rect, and Labels
  and Buttons had zero minimum size, so a VBoxContainer stacked everything on one
  line.
- Prints from a script freed in the same frame (`queue_free`, a scene change)
  were lost.

Open gaps are tracked under the epic `pat-9uuvy` (25 children).

## Roadmap

- `eval`: evaluate a GDScript expression in the running game, as a probe or a tool.
- Saved playtests: `tests/*.playtest.json` run as a suite. Gameplay TDD.
- Annotated screenshots: collision shapes, node names, origins.
- 3D screenshots in the agent path.
- Snapshot/restore of a running game: fork an experiment at frame N.
- Record a human's browser session as a replayable input timeline.
- `export_web`: one call to a playable URL.
