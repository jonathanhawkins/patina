---
name: editor-tour
description: Walk the Patina browser EDITOR itself (not Play) in a real headless Chrome, area by area — scene dock, inspector on 2D/3D/Control/scripted/instanced nodes, FileSystem, the 2D/3D/Script/AssetLib screens, every bottom panel, every menu, the Add Node / Project Settings / Editor Settings dialogs — and report per area a screenshot, console errors and warnings, failed requests, broken images, layout problems (boxes off the window, zero-size panels, overlapping controls, text clipped with no ellipsis), panels that are empty when they must show something, and time to show content. Writes report.md plus a contact-sheet PNG and exits non-zero on hard findings. Use after any change to the editor UI (editor_ui.rs HTML/CSS/JS, editor_server.rs routes the page calls), periodically from the planner, after importing a real game, and when someone says the editor "looks broken", "a panel is empty", "the layout is wrong on my laptop", "the inspector shows nothing". Trigger on: "editor tour", "tour the editor", "check the editor UI", "editor layout", "panel is empty", "editor looks wrong", "UI regression".
argument-hint: <game_dir> [--port P] [--size 1280x720]
---

# Editor Tour

`browser-playtest` judges **Play**; nothing else looked at the editor a person
edits in. Every editor test asserts what its author thought of. This tour
opens each area the way a person exploring the editor would and reports
what is visibly broken. Its first run found two layout bugs no test had
caught (all fixed; tests are in `scripts/tests/editor-layout.test.mjs`):
- the project title ran under the 2D/3D buttons on a 1280px window (the
  span was inline, so its max-width/ellipsis never applied);
- the bottom-panel tab labels wrapped onto 2-3 lines and the 30px tab strip
  cut them in half;
- Chrome warned on every load about the deprecated
  `appearance: slider-vertical` on the audio bus faders.

## Run it

```bash
node scripts/editor-tour.mjs games/<game> --port <yours>                     # 1600x1000
node scripts/editor-tour.mjs games/<game> --port <yours> --size 1280x720     # laptop: most layout bugs show here
```

It (re)starts a **release** editor on the port with `scripts/editor-bg.sh`
(add `--build` after Rust changes; the page HTML is compiled into the binary,
so a CSS/JS edit needs a rebuild too), opens the project's main scene (or
`--scene`), and stops the editor afterwards (`--keep-running` keeps it;
`--no-start` tours one that is already up). Use only your own port. A run
takes about a minute.

Output in `--out` (default a temp dir): `report.md`, `report.json`,
`contact-sheet.png`, one PNG per area. **Look at the contact sheet**: the
checks are heuristics; your eyes catch what they do not (a panel showing the
wrong thing, an icon that is a box).

Exit: 0 no hard findings · 1 hard findings · 2 could not run.

## Reading the findings

**Hard** (exit 1): uncaught page exception, `console.error`, a request
answered 4xx/5xx or failing, a broken image, an area that never opened, a
panel that is missing / `display:none` / zero-size while its area is open,
and an *empty* panel (e.g. the inspector showing fewer than 3 property rows,
or not naming the node just selected).

**Soft** (look, then decide): console warnings, slow areas, and the geometric
checks:
- `overflow` — a visible box runs past the window edge (only the outermost of
  a nest is reported);
- `overlap` — two uncovered controls, or a control and a text label, share
  more than 30% of the smaller box (anything under an open dialog is skipped
  because it is not on top at its centre);
- `clipped-text` — text wider than its own `overflow:hidden` box with no
  ellipsis, wrapped taller than it, or cut by an ancestor that clips
  WITHOUT scrolling; also a one-line input whose VALUE is wider than the
  field ("1.0" showing as "1.0("). Text a scroll container (or an
  overflow:hidden box scrolled from script, like the script editor's line
  gutter) hides is not reported.

If the editor stops answering mid-tour, the tour stops there, says **the
editor died** with the area it was in and the `panicked at` lines from
`$TMPDIR/patina-editor-<port>.log`, and exits 1. That log is copied into
the output as `editor.log` (the next start on the port overwrites it).

A finding that shows in every area (a page-wide one) is listed once, in the
first area, with `[also in N later areas]`.

## Fixing what it finds

- A trivial CSS/JS fix: make it, and prove it with a layout test in
  `scripts/tests/editor-layout.test.mjs`. That test lays out the real
  `EDITOR_HTML` (extracted from `editor_ui.rs`) in headless Chrome with
  scripts off and runs the tour's own checks on it — no editor needed, and it
  fails on behaviour. Red-check it by reverting the CSS. Add a guard for the
  fix's side effect (e.g. "the title still shows in full when there is
  room", "Expand stays on screen when the tab strip scrolls").
- Anything larger: one bead per root cause, with the area, the finding line,
  and the screenshot path.

## Extending the tour

Areas live in `buildAreas()` in `scripts/editor-tour.mjs`: each has the steps
that open it (page expressions; `__tour.click/visible/count/selectNode/rest`
are installed in the page), a `ready` predicate, a `root` to scan, `panels`
that must have size, and `expect` content counts (`{ sel, min, what, equals }`).
The analysis is pure and tested (`scripts/lib/editor-tour-lib.mjs`,
`scripts/tests/editor-tour.test.mjs`): add a test with synthetic rects for
any new check. `collectArea` runs in the page via `toString()`, so it must
not reference anything outside its own body.

## Gotchas

- The inspector keeps the PREVIOUS node's rows until the new ones arrive, so
  "rows exist" is not "ready": the area waits for the header to name the new
  node.
- Bottom panels and menus are toggles: clicking an active tab collapses the
  panel. The tour checks the state before and after clicking.
- Chrome's own `[deprecation]` console warnings (e.g. `slider-vertical`) are
  real but soft.
- Run both sizes after a layout change: 1600x1000 hides most of what a
  laptop shows.
