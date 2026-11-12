# Spark Keeper

A small arena survival game, built through Patina's agent interface
(`docs/AGENT_INTERFACE.md`) to exercise the engine the way a real game does.

You are a glowing orb. Gather sparks for points while gloom drifts in from the
edges. Space sends out a pulse that shoves nearby gloom away (2 s cooldown).
You have three lives; the best score is saved to `user://spark_keeper.cfg`.

Controls: WASD / arrow keys to move, Space to pulse, Space/Enter on menus.

It is a plain Godot 4.6 project, so the same files run in Godot as an oracle:

```bash
# Patina editor + agent interface (from this directory)
../../engine-rs/target/debug/examples/editor --port 8080 scenes/title.tscn
PATINA_PORT=8080 ../../scripts/patina-mcp api '{"method":"POST","path":"/api/project/open","body":{"path":"'"$PWD"'"}}'

# a full loop: start, play, lose every life, back to the title
PATINA_PORT=8080 ../../scripts/patina-mcp play '{"frames":1100,"inputs":[{"action":"pulse","at":3},{"action":"pulse","at":1000}],"probes":[{"node":"/root/Game","props":["lives","score"],"every":300}],"screenshots":[300,950]}'

# real Godot
/Applications/Godot.app/Contents/MacOS/Godot --headless --path . --quit-after 400
```

Exercised: autoload singleton with signals, scene instancing at runtime
(`preload` + `instantiate`), scene changes, CharacterBody2D movement, Area2D
overlap, collision layers, Timers, Tweens, CPUParticles2D, `_draw()`, Polygon2D,
Line2D, Sprite2D with a PNG, a canvas shader, Control layout (VBox/HBox,
anchors), ProgressBar, ConfigFile persistence, a seeded RandomNumberGenerator,
groups, `is`/`as`, and the input map.

Workarounds for gaps still open under `pat-9uuvy` are marked where they live:
`rng.set_seed` (property assignment on native objects), the `FileAccess.file_exists`
guard before `ConfigFile.load`, `await create_timer` instead of `tween_callback`,
`call_deferred("_method")` instead of a native method as a Callable, and a
Polygon2D rather than a ColorRect for the background (Control draw order).
