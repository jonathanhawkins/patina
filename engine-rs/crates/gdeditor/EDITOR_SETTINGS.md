# Editor Settings & Preferences Surface

Documents the current configurable settings exposed by the editor.

## Current Settings (Supported)

| Setting       | How to set                  | Default     | Notes                              |
|---------------|-----------------------------|-------------|------------------------------------|
| Port          | `--port <PORT>` CLI flag    | `8080`      | TCP port the HTTP server listens on |
| Scene path    | Positional CLI argument      | `""`        | Path to `.tscn` file to open on start |
| Viewport size | Hardcoded in initial state   | `640 × 480` | Canvas render dimensions            |

## Future Settings (Deferred — editor maintenance only until runtime parity exits)

The following settings are **not yet implemented** and are deferred until the
engine reaches ≥98% oracle parity across all supported scenes:

- **Theme** — light/dark/custom color scheme for the editor shell
- **Auto-save** — periodic scene auto-save interval
- **Layout** — panel positions, sizes, and visibility (scene tree, inspector, etc.)
- **Snap grid** — snap-to-grid interval for viewport placement
- **Keybindings** — remappable editor keyboard shortcuts

## Scope Note

Per `AGENTS.md`, no new editor feature work is permitted until runtime parity
exits are met. Settings listed above as "future" must not be implemented as
production features; placeholder stubs are acceptable for testing purposes only.
