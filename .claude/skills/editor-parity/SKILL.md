---
name: editor-parity
description: Compare the Patina editor against Godot 4.6.1 visually. Takes screenshots of both editors, identifies UX gaps, and creates beads for differences.
argument-hint: [area-to-compare]
trigger: "editor parity", "compare editors", "compare to godot", "visual parity", "editor diff", "how does it compare"
---

# Editor Parity Checker

Visually compare the Patina editor against Godot 4.6.1 and create beads for differences.

## How It Works

Claude uses multimodal vision to semantically compare both editors — no pixel-diffing, just intelligent visual analysis of layout, controls, styling, and behavior.

## Steps

### 1. Ensure both editors are visible

Check browser tabs for the Patina editor:
```
mcp__claude-in-chrome__tabs_context_mcp
```

If Patina editor isn't open, navigate to `http://localhost:8080/editor` in a new tab.

Ask the user to have Godot 4.6.1 open to the same view/state if it's not already visible. The user may also provide screenshots directly.

### 2. Capture Patina editor screenshot

Use Claude-in-Chrome to screenshot the Patina editor tab:
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
2. **Godot is in another tab** — screenshot it via Claude-in-Chrome
3. **Reference from memory** — use knowledge of Godot 4.6.1's editor layout (Claude has trained on Godot editor screenshots extensively)

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
- **P2 — Missing feature** (feature exists in Godot but absent in Patina)
- **P3 — Visual polish** (works but looks different from Godot)

### 6. Create beads for gaps

For each P1 and P2 gap, check if a bead already exists:
```bash
br search "<keyword>" 2>/dev/null
```

If no existing bead, create one:
```bash
br sync --rebuild 2>&1 | tail -1
br create "<title>" -p <priority> --type <bug|feature> --labels editor
```

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

## Tips

- Compare the SAME scene in both editors for meaningful results
- Focus on one area at a time (scene tree, inspector, viewport) for detailed analysis
- Run this skill periodically as editor work progresses to track convergence
- The `$ARGUMENTS` can specify which area to focus on: "scene tree", "inspector", "viewport", "toolbar", "filesystem", or "full" for everything
