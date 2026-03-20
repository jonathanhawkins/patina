# Editor Settings & Preferences Surface

Documents all configurable settings exposed by the Patina editor.

## CLI Settings

| Setting    | How to set               | Default     | Notes                                 |
|------------|--------------------------|-------------|---------------------------------------|
| Port       | `--port <PORT>` CLI flag | `8080`      | TCP port the HTTP server listens on   |
| Scene path | Positional CLI argument  | `""`        | Path to `.tscn` file to open on start |

## Editor Display Settings (Settings Dialog)

Accessible via the gear button (`btn-settings`) or Edit > Preferences menu item.

| Setting         | Element ID         | Type    | Default  | Range / Values                    |
|-----------------|--------------------|---------|----------|-----------------------------------|
| Grid Snap       | `set-grid-snap`    | bool    | `false`  | on / off                          |
| Snap Size       | `set-snap-size`    | int     | `16`     | 1 - 256 px                        |
| Grid Visible    | `set-grid-visible` | bool    | `true`   | on / off                          |
| Rulers Visible  | `set-rulers-visible`| bool   | `true`   | on / off                          |
| Font Size       | `set-font-size`    | int     | `13`     | 8 - 32 px                         |
| Theme           | `set-theme`        | enum    | `"dark"` | `"dark"`, `"light"`               |
| Physics FPS     | `set-physics-fps`  | int     | `60`     | 1 - 240                           |

## Viewport State (Runtime)

These are not persisted settings but live state that affects the viewport.

| Property        | Description                          | Default        |
|-----------------|--------------------------------------|----------------|
| Viewport Width  | Canvas render width                  | `640`          |
| Viewport Height | Canvas render height                 | `480`          |
| Viewport Zoom   | Current zoom level (1.0 = 100%)      | `1.0`          |
| Viewport Pan    | Current pan offset (x, y) in pixels  | `(0.0, 0.0)`   |
| Viewport Mode   | Tool mode for viewport interaction   | `Select`       |
| Editor Mode     | Active mode string                   | `"select"`     |

## Project Settings (API)

Accessible via `GET /api/project_settings` and `POST /api/project_settings`.
Stored in `project_settings.json` in the project root.

| Setting            | Type   | Default       | Description                            |
|--------------------|--------|---------------|----------------------------------------|
| project_name       | String | `""`          | Display name of the project            |
| main_scene         | String | `""`          | Path to the main scene file            |
| window_width       | int    | `1024`        | Game window width                      |
| window_height      | int    | `600`         | Game window height                     |
| physics_fps        | int    | `60`          | Target physics frames per second       |
| gravity            | float  | `980.0`       | Default gravity for 2D physics         |
| default_gravity_vector | Vec2 | `(0, 1)`   | Direction of default gravity           |

## Inspector Features

| Feature              | Description                                              |
|----------------------|----------------------------------------------------------|
| Property search      | Filter properties by name in the inspector panel         |
| Multi-object editing | Edit shared properties across multiple selected nodes    |
| Export vars          | Display `@export` variables parsed from attached scripts |
| History navigation   | Back/forward buttons to revisit previously inspected nodes |
| Resource info        | Resource type and path displayed in inspector toolbar    |

## Output Panel

| Setting                 | Type | Default | Description                           |
|-------------------------|------|---------|---------------------------------------|
| MAX_FRONTEND_LOG_ENTRIES| int  | `200`   | Max log entries displayed in output panel before oldest are pruned |

## Scope Note

Per `AGENTS.md`, no new editor feature work is permitted until runtime parity
exits are met. Settings listed here reflect the current implementation state.
Any additions beyond maintenance fixes require gate clearance.
