# Editor Parity — Source-Driven Visual Fidelity Methodology

**Status:** active (2026-06-18). Owner: planner + `/editor-parity`.

## Why this exists

The goal is that a user **cannot tell Patina's editor apart from Godot's** — same
chrome, same labels, same icons, in the same places. We have Godot's full source
in `upstream/godot/`, so parity is not a guessing game: it is a 1:1 transcription
of what Godot's editor *constructs*.

The planner was missing UX gaps because it only seeded beads from hand-authored
maps that enumerated **functional features** ("implement file ops", "implement
favorites + context menu"). It never read `upstream/godot/`, and its acceptance
tests asserted **behavior**, not **structure**. So permanent visual chrome that
Godot always shows — the `★ Favorites:` header, the path bar, the "Filter Files"
box, the sort/split toggles — was never enumerated, never beaded, and a passing
"favorites works" test could coexist with the header being completely absent.

This methodology closes that gap: **gap discovery reads the Godot source and
emits structure-asserting beads.**

## Core principles

1. **The Godot C++ source is ground truth.** For any dock/panel/dialog, open the
   corresponding file under `upstream/godot/editor/` and read its constructor and
   `_ready()` / `_notification(NOTIFICATION_*)` handlers. That code *is* the spec.

2. **Permanent chrome is a P2 missing-feature, not P3 polish.** A widget Godot
   instantiates **unconditionally** (in the constructor, not behind a state check)
   is structural parity. If Patina doesn't render it, that is a *missing feature*
   (P2) — never deprioritized as "visual polish" (P3). P3 is reserved for chrome
   that is present but styled/positioned slightly differently.

3. **Every visual-fidelity bead carries a DOM/structure acceptance test.** The
   test boots `EditorServerHandle`, GETs `/editor`, and asserts the served HTML
   contains the element — its id, its verbatim Godot label text (the `TTRC(...)`
   string), and its icon glyph. Structure is testable; "looks the same" is not.
   This is what makes a criterion *truly* satisfied rather than box-ticked.

4. **Enumerate at widget granularity, not feature granularity.** One bead per
   always-present widget (or tight group), not one bead per "dock". "Implement the
   FileSystem dock" hides a dozen missing widgets; "Render the always-present
   `★ Favorites:` section header" cannot.

## The enumeration procedure (repeatable)

For each editor surface:

1. **Locate the source.** e.g. FileSystem dock → `upstream/godot/editor/docks/filesystem_dock.cpp`.
2. **Read the constructor + ready/notification handlers.** Grep for `memnew(`,
   `add_child(`, `set_text(`, `set_placeholder(`, `TTRC(` / `TTR(`, `set_button_icon(`,
   `get_editor_theme_icon(`.
3. **Classify each widget** as **permanent** (created unconditionally) or
   **conditional** (created/shown behind an `if`/state). Permanent widgets are the
   visual-fidelity surface.
4. **Diff against Patina's served HTML** (`editor_ui.rs` → `/editor`). For each
   permanent widget, is there a counterpart element (id/text/icon)?
5. **Emit beads:**
   - Missing permanent widget → **P2** bead, label `editor`, with a DOM acceptance
     test `(test: \`name\`)`.
   - Present but wrong (text/icon/order/style) → **P3** bead with a DOM test.
   - Conditional/behavioral depth (the *logic* behind the widget) → its own
     behavioral bead (existing functional methodology).
6. **Record the enumeration** in the surface's visual-fidelity map so it is
   auditable and the planner can seed from it.

## Worked example — FileSystem dock

Source: `upstream/godot/editor/docks/filesystem_dock.cpp`, `FileSystemDock::FileSystemDock()`.

Permanent widgets Godot's constructor always creates (verbatim):

| Godot widget (source) | Always-present? | Patina (before) | Bead |
|---|---|---|---|
| `button_hist_prev` "Go to previous selected folder/file." | yes | missing | `fs-chrome-nav-history` |
| `button_hist_next` "Go to next selected folder/file." | yes | missing | `fs-chrome-nav-history` |
| `current_path_line_edit` (Path) | yes | missing | `fs-chrome-path-bar` |
| `button_toggle_display_mode` "Change Split Mode" | yes | missing | `fs-chrome-split-toggle` |
| `tree_search_box` placeholder `TTRC("Filter Files")` | yes | missing | `fs-chrome-filter-box` |
| `tree_button_sort` | yes | missing | `fs-chrome-sort-button` |
| `favorites_item` text `TTRC("Favorites:")` + Favorites star icon | yes (even when empty) | **missing** | `fs-chrome-favorites-header` |

Every row above is **P2** (Godot shows it unconditionally), each tested by a
served-DOM assertion in `editor_filesystem_chrome_test.rs`. The *logic* behind
them (real favoriting persistence, split-mode layout switch, path-typing
navigation, context-menu actions, import status) stays as separate behavioral
beads — see `prd/EDITOR_PARITY_FILESYSTEM_DOCK_MAP.md`.

## How this plugs into the planner

- The planner seeds beads from execution maps. Author a
  `EDITOR_PARITY_VISUAL_FIDELITY_MAP.md` (+ `_EXIT.md`) per the format above and
  wire it as a `[[phase]]` in `.orchestrator/planner.toml` with
  `analysis.source = "criteria"` and the DOM test binary in `test_binaries`. The
  planner then ticks each `- [ ]` as its DOM test passes and seeds rework beads
  for the rest — exactly like the functional lanes, but for structure.
- `/editor-parity` runs the enumeration procedure (it now reads `upstream/godot/`
  first, not just vision) and creates the missing-widget beads at P2.

## Surfaces to enumerate (backlog)

Each maps to a file under `upstream/godot/editor/`:

- [x] FileSystem dock — `docks/filesystem_dock.cpp` (worked example above)
- [ ] Scene tree dock — `docks/scene_tree_dock.cpp` / `gui/scene_tree_editor.cpp`
- [ ] Inspector dock — `docks/inspector_dock.cpp` / `editor/inspector/editor_inspector.cpp`
- [ ] Node dock (Signals/Groups) — `docks/node_dock.cpp`, `connections_dialog.cpp`, `groups_editor.cpp`
- [ ] Import dock — `docks/import_dock.cpp`
- [ ] History dock — `docks/history_dock.cpp`
- [x] Script editor tab — `editor/script/script_editor_plugin.cpp` (menu_hb toolbar: File/Search/Debug, Online Docs, Search Help, history arrows, make-floating; scripts_vbox/overview_vbox left column). Chrome implemented; behavioral depth → pat-kgfgf/pat-f64ui/pat-nrwy2.
- [ ] Bottom panels (Output/Debugger/Audio/Animation/Shader) — `editor/*`
- [ ] Top menu bar + run toolbar — `editor/editor_node.cpp` (`_build_menu`, run bar)
- [ ] Main viewport toolbars — `editor/plugins/canvas_item_editor_plugin.cpp`, `node_3d_editor_plugin.cpp`
