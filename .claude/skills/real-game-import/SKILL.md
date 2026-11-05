---
name: real-game-import
description: Import a real open-source Godot game into games/ unmodified, find every engine bug it exposes, and fix them in parallel. The loop is scene-audit, browser playtest, one bead per root cause, worktree subagents on their own editor ports, merge, the full suite, then the gate. Use when adding a new real-game fixture, when someone says "try a real game", "import a demo", "does game X run on Patina", or when synthetic tests are green but real games still break. Trigger on: "import a game", "real game", "godot demo project", "new game fixture", "scene audit", "what breaks in game X".
argument-hint: [game-name] [upstream path, e.g. 3d/platformer]
---

# Real-game import

Synthetic tests check what someone thought to test. A real game checks what
games actually do. dodge_the_creeps found about 20 engine bugs the suite had
missed. platformer_3d found binary `.scn` loading, glb-inherited scenes,
missing `RenderingServer`/`DisplayServer` natives, and 3D Play drawing the
editor overlay.

## 1. Pick and import (unmodified)

- Source: the official demos (MIT) are the default, at
  https://github.com/godotengine/godot-demo-projects. Others are fine if the
  license allows redistribution (MIT, CC0, Apache, BSD). Check the license
  before copying, and keep the art and audio credits.
- Sparse-checkout just the one project, outside the repo:
  ```bash
  git clone --depth 1 --filter=blob:none --sparse https://github.com/godotengine/godot-demo-projects /tmp/gdp
  git -C /tmp/gdp sparse-checkout set 3d/platformer
  git -C /tmp/gdp rev-parse HEAD                      # for SOURCE.md
  cp -R /tmp/gdp/3d/platformer games/<game>           # drop .godot/ if present
  ```
- Copy it UNMODIFIED. A game you patched to work proves nothing. The only
  files you may add are `SOURCE.md` and `playtest.json`. `SOURCE.md` names the
  upstream URL and path, the commit, the license, and where the asset credits
  live (see `games/platformer_3d/SOURCE.md`).

## 2. Scene audit (seconds, no browser)

```bash
scripts/rust_task.sh build --release -p patina-runner      # if missing/stale
node scripts/scene-audit.mjs games/<game>                  # add --json for tooling
```

The audit loads every `.tscn`/`.scn` through the release runner, with
`--script-output --res-root <game>`, and reports per scene:

- load errors, with the runner's own message
- node count
- **placeholder instances**: an `instance=` node that came out as an empty
  plain `Node`, because the loader swallowed a failure
- **missing methods**: a native the game calls that Patina lacks
- other script errors
- types ClassDB does not register

Exit 1 means there is a finding. Binary resources are listed as notes.
On platformer_3d:

```
  FAIL  stage/grid_map.scn  did not load
          - load error: Error reading 'stage/grid_map.scn': stream did not contain valid UTF-8 (binary-compressed file)
  FAIL  stage/stage.tscn    14 nodes
          - placeholder: /root/Stage/GridMap <res://stage/grid_map.scn> became an empty Node
          - missing method: RenderingServer.get_current_rendering_method
```

The 2D games audit clean, so a finding is real. For the LIVE tree, run it
against your own editor:
`node scripts/scene-audit.mjs games/<game> --port <yours>`. That plays for
about 1.5 s, reads the running tree and the game's errors, then stops.

⚠️ A runner built before `--res-root` reports every instance in a sub-folder
scene as a placeholder. The audit warns when the runner lacks a flag. Rebuild
it; do not trust the findings.

## 3. Play it in the browser

Write `games/<game>/playtest.json` in the gate's format (see
`games/platformer/playtest.json`). Use the scene, keys from the game's
`[input]` map, `watch` on the player's position, and `expect` rules saying the
position must change after each movement key. Then:

```bash
scripts/editor-bg.sh start --release --build --port <yours> games/<game> <main scene>
node scripts/playtest-gate.mjs --games <game> --port-base <yours>
```

Use the `browser-playtest` skill for anything the gate flags: fps, overlays,
dead controls. A new game is EXPECTED to fail here. The point is to get a
clear functional failure, not a crash. Look at the end screenshot yourself.

## 4. One bead per ROOT CAUSE

Group the findings by cause, not by symptom. Four `Enemy*` placeholders and a
game.tscn load error are one bug: enemy.tscn's glb-inherited parent path.
Check that no open bead already covers it (`br list --label real-game`), then:

```bash
br create -t bug -p 1 -l real-game,<game> \
  -d "<finding verbatim> · repro: node scripts/scene-audit.mjs games/<game> · files: <suspects>" \
  "<game>: <root cause in one line>"
```

Put the exact audit or gate line and a one-command reproduction in the body.

**Attach a Godot reference picture/trace to every visual or behaviour bead.**
Judge against what real Godot does, not against the upstream screenshot by eye.
Use the `godot-reference` skill (`.claude/skills/godot-reference/SKILL.md`):

```bash
node scripts/compare-to-godot.mjs games/<game> --port <yours> --out .scratch/cmp-<game>
```

Paste the trace table (first divergence frame, max error) and the
`diff/fNNNNN-side.png` path (Godot | Patina | heat map) into the bead.

## 5. Fix in parallel: worktree subagents, one port each

Dispatch one subagent per bead with `isolation: "worktree"`. Give each one its
OWN editor port (for example 9440, 9441, …). Say which ports are off-limits
(other agents' ports, the person's 8080), and that it must use
`scripts/editor-bg.sh stop --port N`, never a stop without `--port`. Each
brief includes:

- the bead id and the finding verbatim
- the repro command
- the rule from CLAUDE.md: every fix needs a test that fails without it
- `scripts/rust_task.sh nextest run -p <crate>` with
  `NEXTEST_TEST_THREADS<=2`, and NOT the whole workspace
- commit on the worktree branch

Tell agents that touch the same file (for example the Play render path) about
each other. Keep tool changes surgical.

## 6. Merge, then the full suite, then the gate

- Merge the worktree branches one at a time and resolve conflicts by reading
  both sides.
- Run the full suite once on the merged tree:
  `cargo nextest run --workspace` (via `scripts/rust_task.sh`). Grep the output
  for both `Summary` and `could not compile`, because one compile break runs
  ZERO tests and still exits 0. Compare the failures against the known red
  list before blaming the merge.
- Re-run `node scripts/scene-audit.mjs games/<game>` and the gate for EVERY
  game (`--games <g> --port-base <yours>`, one at a time). A fix for the new
  game must not turn an old game red. Close a bead only when its finding has
  gone from the audit or gate output.

## Gotchas

- Godot's `res://` is the project root. The runner's loose-scene default is
  the scene's own folder (the engine fixtures rely on it), so tools must pass
  `--res-root`.
- Judge Play on a RELEASE editor. A debug build renders about 10x slower.
- The MCP `tree` outline stops at a node budget of about 400. The audit reads
  the JSON runtime tree and asks the outline only for each suspect's source.
