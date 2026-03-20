# Editor Settings & Preferences Surface

Documents all configurable settings exposed by the Patina editor.

## CLI Settings

| Setting    | How to set               | Default     | Notes                                 |
|------------|--------------------------|-------------|---------------------------------------|
| Port       | `--port <PORT>` CLI flag | `8080`      | TCP port the HTTP server listens on   |
| Scene path | Positional CLI argument  | `""`        | Path to `.tscn` file to open on start |

## Editor Display Settings (Settings Dialog)

Accessible via the gear button (`btn-settings`) or Edit > Preferences menu item.

| Setting         | Element ID         | Type    | Default    | Range / Values                    |
|-----------------|--------------------|---------|------------|-----------------------------------|
| Grid Snap       | `set-grid-snap`    | bool    | `false`    | on / off                          |
| Snap Size       | `set-snap-size`    | enum    | `8`        | `8`, `16`, `32`, `64`             |
| Grid Visible    | `set-grid-visible` | bool    | `true`     | on / off                          |
| Rulers Visible  | `set-rulers-visible`| bool   | `true`     | on / off                          |
| Font Size       | `set-font-size`    | enum    | `"medium"` | `"small"`, `"medium"`, `"large"`  |
| Theme           | `set-theme`        | enum    | `"dark"`   | `"dark"`, `"light"`               |
| Physics FPS     | `set-physics-fps`  | enum    | `60`       | `30`, `60`, `120`                 |

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

Accessible via the Project > Project Settings menu or `GET /api/project_settings` and `POST /api/project_settings`.
Stored in `EditorState` fields on the server.

| Setting            | Element ID       | Type   | Default          | Range / Description                    |
|--------------------|------------------|--------|------------------|----------------------------------------|
| Project Name       | `pset-name`      | String | `"New Project"`  | Display name of the project            |
| Resolution W       | `pset-res-w`     | int    | `1152`           | 1 - 7680, game window width            |
| Resolution H       | `pset-res-h`     | int    | `648`            | 1 - 4320, game window height           |
| Physics FPS        | `pset-physics-fps`| enum  | `60`             | `30`, `60`, `120`                      |
| Gravity            | `pset-gravity`   | float  | `980.0`          | Default gravity for 2D physics         |
| Main Scene         | `pset-main-scene`| String | `""`             | Path to the main scene file (res://...) |

## Inspector Features

| Feature              | Description                                              |
|----------------------|----------------------------------------------------------|
| Property search      | Filter properties by name in the inspector panel         |
| Multi-object editing | Edit shared properties across multiple selected nodes    |
| Export vars          | Display `@export` variables parsed from attached scripts |
| History navigation   | Back/forward buttons (`inspectorBack`/`inspectorForward`) to revisit previously inspected nodes. State tracked in `inspectorHistory` array, pushed via `pushInspectorHistory()`. UI element: `insp-history`. |
| Resource info        | Resource type and path displayed in inspector toolbar    |
| Resource toolbar     | Inspector toolbar shows resource type, path, and navigation controls. `GET /api/selected` returns full property list for the currently selected node including resource references. |

## FileSystem Dock

| Feature              | API Endpoint              | Description                                              |
|----------------------|---------------------------|----------------------------------------------------------|
| Browse files         | `GET /api/filesystem`     | Scan project directory tree with size and file type info  |
| Rename               | `POST /api/filesystem/rename` | Rename a file or folder (body: `old_path`, `new_name`) |
| Delete               | `POST /api/filesystem/delete` | Delete a file or empty folder (body: `path`)           |
| Create folder        | `POST /api/filesystem/mkdir`  | Create a new directory (body: `path`)                  |

Right-click any item in the FileSystem dock to access rename, delete, and (for folders) new folder actions.

## Scene Tree Features

| Feature              | Description                                              |
|----------------------|----------------------------------------------------------|
| Multi-node drag      | Drag multiple selected nodes to reorder or reparent      |
| Script badge         | Script icon shown on nodes with attached scripts         |
| Signal badge         | Lightning arrow icon on nodes with connected signals     |
| Group badge          | `[G]` badge on nodes belonging to groups                 |
| Favorites/Recent     | Create Node dialog shows favorite and recently used types |

## Output Panel

| Setting                 | Type | Default | Description                           |
|-------------------------|------|---------|---------------------------------------|
| MAX_FRONTEND_LOG_ENTRIES| int  | `200`   | Max log entries displayed in output panel before oldest are pruned |

## Server-Side Log Cap

| Setting           | Location           | Type | Default | Description                              |
|-------------------|--------------------|------|---------|------------------------------------------|
| MAX_LOG_ENTRIES   | `editor_server.rs` | int  | `100`   | Max log entries kept in server memory     |

## Scope Note

As of 2026-03-19, the editor feature gate is MET. Settings listed here reflect
the current implementation state.
