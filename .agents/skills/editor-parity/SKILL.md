---
name: editor-parity
description: Compare the Patina editor against Godot 4.6.1 visually. Takes screenshots of both editors, identifies UX gaps, and creates beads for differences.
argument-hint: [area-to-compare]
trigger: "editor parity", "compare editors", "compare to godot", "visual parity", "editor diff", "how does it compare"
---

# Editor Parity Checker

Compare the Patina editor against Godot 4.6.1 and create beads for differences.

## How It Works

This is **source-driven first, vision second**. The goal is a 1:1 copy, and we
have Godot's source in `upstream/godot/` — so ground truth is the C++ that builds
Godot's editor, not a screenshot. You enumerate the widgets Godot *constructs* for
the area, diff them against Patina's served HTML, and only then use multimodal
vision to catch styling/positioning differences. See
`prd/EDITOR_PARITY_VISUAL_FIDELITY_METHODOLOGY.md` for the full rationale.

**Permanent chrome missing in Patina is a P2 missing-feature, never P3 polish.**
A widget Godot instantiates unconditionally (the `★ Favorites:` header, the path
bar, the "Filter Files" box, …) is structural parity — if Patina doesn't render
it, bead it at P2 with a DOM acceptance test. This is the gap the old
vision-only flow kept missing.

## Steps

### 0. Enumerate from the Godot source (ground truth — do this first)

Before screenshotting anything, open the Godot source for the area and list the
widgets it builds. Map the area to its file under `upstream/godot/editor/`:

| Area | Source file |
|------|-------------|
| filesystem | `docks/filesystem_dock.cpp` |
| scene tree | `docks/scene_tree_dock.cpp`, `gui/scene_tree_editor.cpp` |
| inspector | `docks/inspector_dock.cpp`, `inspector/editor_inspector.cpp` |
| signals/groups | `docks/node_dock.cpp`, `connections_dialog.cpp`, `groups_editor.cpp` |
| toolbar / menus / run bar | `editor_node.cpp` |
| viewport | `plugins/canvas_item_editor_plugin.cpp`, `plugins/node_3d_editor_plugin.cpp` |

Read the constructor + `_ready()` / `_notification()` and grep for
`memnew(`, `set_text(`, `set_placeholder(`, `TTRC(` / `TTR(`, `set_button_icon(`.
Write down every widget the code creates **unconditionally** (permanent chrome)
with its verbatim label/placeholder text and icon. This list is the parity spec.

Then diff it against Patina's served HTML (`GET /editor`, or read
`engine-rs/crates/gdeditor/src/editor_ui.rs`). Each permanent widget with no
counterpart is a P2 gap (step 5/6 below).

### 1. Ensure both editors are visible

Check browser tabs for the Patina editor:
```
mcp__claude-in-chrome__tabs_context_mcp
```

If Patina editor isn't open, navigate to `http://localhost:8080/editor` in a new tab.

Ask the user to have Godot 4.6.1 open to the same view/state if it's not already visible. The user may also provide screenshots directly.

### 2. Capture Patina editor screenshot

Use Codex-in-Chrome to screenshot the Patina editor tab:
```
mcp__claude-in-chrome__computer action=screenshot tabId=<patina_tab_id>
```

If `$ARGUMENTS` specifies an area (e.g. "inspector", "scene tree", "viewport", "toolbar"), zoom into that region:
```
mcp__claude-in-chrome__computer action=zoom tabId=<patina_tab_id> region=[x0,y0,x1,y1]
```

### 3. Get Godot reference

Options (try in order):
1. **User provides screenshot** — if the user attached an image of Godot, use that
2. **Godot is in another tab** — screenshot it via Codex-in-Chrome
3. **Reference from memory** — use knowledge of Godot 4.6.1's editor layout (Codex has trained on Godot editor screenshots extensively)

### 4. Visual comparison

Compare the two editors across these dimensions, reporting each as a table:

#### A. Layout & Panels
| Element | Godot 4.6.1 | Patina | Match? |
|---------|-------------|--------|--------|
| Scene tree position | left dock | ? | |
| Inspector position | right dock | ? | |
| Viewport position | center | ? | |
| Bottom panels | Output/Debugger/Audio/Animation/Shader | ? | |
| FileSystem dock | left bottom with full browser | ? | |
| Toolbar | top with icon buttons | ? | |
| Menu bar | Scene/Import above tree | ? | |

#### B. Scene Tree
| Feature | Godot 4.6.1 | Patina | Match? |
|---------|-------------|--------|--------|
| Node type icons | colored by class (blue=Node2D, etc.) | ? | |
| Script indicator | scroll icon on scripted nodes | ? | |
| Warning indicator | yellow triangle | ? | |
| Visibility toggle | eye icon | ? | |
| Lock toggle | lock icon | ? | |
| Context menu | full right-click menu | ? | |
| Drag-drop reorder | yes | ? | |
| Multi-select | shift+ctrl click | ? | |

#### C. Inspector
| Feature | Godot 4.6.1 | Patina | Match? |
|---------|-------------|--------|--------|
| Property categories | grouped (Node, Node2D, CanvasItem) | ? | |
| Property editors | type-specific widgets | ? | |
| Resource toolbar | new/open/save/back/forward | ? | |
| Node class header | shows class name + icon | ? | |
| Filter properties | search box | ? | |

#### D. Viewport
| Feature | Godot 4.6.1 | Patina | Match? |
|---------|-------------|--------|--------|
| Grid rendering | fine grid with major lines | ? | |
| Node rendering | class-specific (sprites, collision shapes) | ? | |
| Selection highlight | orange outline + handles | ? | |
| Transform gizmo | move/rotate/scale handles | ? | |
| Zoom controls | mouse wheel + UI buttons | ? | |
| Rulers | top + left rulers | ? | |
| Node labels | readable names below nodes | ? | |

#### E. Styling
| Element | Godot 4.6.1 | Patina | Match? |
|---------|-------------|--------|--------|
| Color scheme | dark gray (#2d2d2d ish) | ? | |
| Font | system sans-serif | ? | |
| Button style | flat with hover highlights | ? | |
| Panel borders | subtle 1px separators | ? | |
| Icon style | monochrome with class colors | ? | |

### 5. Prioritize gaps

Categorize each difference as:
- **P1 — Broken** (functionality doesn't work — e.g. selection broken, garbled text)
- **P2 — Missing feature** (feature exists in Godot but absent in Patina). **This
  includes any permanent chrome from step 0** — a widget Godot's constructor
  always builds (header, label, toolbar button, filter box, …) that Patina does
  not render. Do NOT demote always-present chrome to P3 because it "only looks
  different"; if it is absent it is missing (P2).
- **P3 — Visual polish** (the widget IS present but styled/positioned/labeled
  slightly differently from Godot)

### 6. Create beads for gaps

For each P1 and P2 gap, check if a bead already exists. Run this EXACT command and extractor — shell only, no python wrappers, no fuzzy matching:

```bash
br search "TITLE" --status open --status in_progress --status closed --json --no-auto-import --allow-stale | jq -r 'length'
```

NOTE: `br search` (and `br ready`) return a **bare JSON array** in br 0.1.x, so the
count extractor is `jq -r 'length'` — NOT `jq -r '.issues | length'` (that yields
`null` and silently breaks the rule below, making every gap look new).

**Selection rule**: The output is a single integer N (count of matches across all statuses).
- If `N >= 1` → SKIP this gap. Do NOT create a bead. Do NOT inspect titles. Do NOT decide whether the matches are "close enough" or "really the same".
- If `N == 0` → create the bead with the command below.

Do NOT pipe `br search` into python. Do NOT define your own match predicate. Do NOT compare titles yourself — `br search` already handles substring/FTS matching, any hit means a duplicate exists.

Create the bead AND set concrete acceptance criteria naming a **DOM/structure
test** (the methodology requires every visual-fidelity bead to be testable — a
served-HTML assertion that the element/label/icon is present, e.g. in
`editor_filesystem_chrome_test.rs`). A bead with null acceptance criteria is
forbidden (workers invent their own scope — see the planner's hard rules):

```bash
br sync --rebuild 2>&1 | tail -1
ID=$(br create "<title>" -p <priority> --type <bug|feature> --labels editor --json | jq -r '.id')
br update "$ID" --acceptance-criteria "Served /editor HTML renders <element> with <label/icon> matching Godot's <widget> (always present). (test: \`<dom_test_name>\`)"
```

For a permanent-chrome gap, phrase the title as the widget, not the feature:
"Render the always-present `★ Favorites:` section header in the FileSystem dock",
not "Implement favorites". The acceptance test asserts the served DOM, so the
bead can't be box-ticked while the widget is still absent.

### 7. Summary

Present a final report:
```
## Editor Parity Report — [area]

### Score: N/M features matching

### Gaps Found
| Priority | ID | Gap |
|----------|-----|-----|
| P1 | pat-xxx | ... |
| P2 | pat-yyy | ... |

### Already Tracked
- pat-aaa: ...
- pat-bbb: ...

### Next Steps
- [prioritized list of what to fix first]
```

## Hard Rules

- NEVER wrap `br search` (or `br ready`, `br count`, `br list`) in a python (or any) filter that defines its own match/dedup predicate. Use the shown `jq -r 'length'` extractor (br search/ready return a bare array) and apply the integer rule (`>= 1` skip, `== 0` create).
- NEVER inspect candidate titles to decide if a `br search` hit is "really" a duplicate — any hit is a duplicate, period.
- NEVER skip the dedup query and create beads based on your own judgment of whether a gap is "new enough".

## Tips

- Compare the SAME scene in both editors for meaningful results
- Focus on one area at a time (scene tree, inspector, viewport) for detailed analysis
- Run this skill periodically as editor work progresses to track convergence
- The `$ARGUMENTS` can specify which area to focus on: "scene tree", "inspector", "viewport", "toolbar", "filesystem", or "full" for everything
