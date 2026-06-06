# Editor Parity Beads

This backlog is derived from `prd/GODOT_EDITOR_FEATURES.md`.

Purpose:

- capture the major editor feature gaps now
- keep them blocked until runtime parity is reconciled
- give future editor work a clean lane structure instead of ad hoc UI bugs

## Execution Rule

These beads are backlog only until runtime parity closes.

Block editor feature work behind:

- `pat-4ap` Audit: reconcile docs, tests, and bead state before repin

That keeps runtime/oracle closure as the active priority while preserving the editor roadmap.

## Editor Lanes

1. `Scene Tree parity: node operations and hierarchy workflows`
   Source: Scene Tree dock node operations, hierarchy features, context menu, groups

2. `Scene Tree parity: indicators, badges, and selection state`
   Source: visual indicators, lock/visibility, instancing/script/group/signal/unique-name markers

3. `Inspector parity: resource toolbar, history, and object navigation`
   Source: inspector top toolbar, header, back/forward, history, sub-resource navigation

4. `Inspector parity: core property editing and interaction`
   Source: property editors, drag-to-adjust, revert/defaults, linked values, copy/paste paths

5. `Inspector parity: advanced property organization and exported script fields`
   Source: favorites, grouped exports, sub-resource inline editing, hints/categories

6. `Viewport parity: selection modes, zoom/pan, and viewport controls`
   Source: 2D viewport toolbar modes, zoom, pan, framing, overlap selection, locked-node behavior

7. `Viewport parity: transform gizmos and pivot workflows`
   Source: move/rotate/scale gizmos, origin marker, local/global toggle

8. `Viewport parity: snapping, guides, rulers, grid, and canvas overlays`
   Source: snap config, smart snap, guides, rulers, origin, navigation/y-sort/viewport overlays

9. `Top bar parity: scene tabs, run controls, and editor mode switching`
   Source: scene tabs, run/play/pause/stop, 2D/3D/Script/Game/AssetLib modes

10. `Menu parity: scene/project/debug/editor/help actions`
    Source: top-level menus and global undo/redo surfaces

11. `Create Node dialog parity for 2D workflows`
    Source: searchable node dialog, favorites/recent, 2D node catalog and common helper nodes

12. `Bottom panels parity: output, debugger, monitors, audio buses, shader editor`
    Source: output/debugger/audio/shader panels and their core interactions

13. `Script editor parity: core editing features`
    Source: syntax highlighting, completion, markers, folding, caret tools, minimap, diagnostics

14. `Script editor parity: search, navigation, debugging, and script panel`
    Source: find/replace, go-to, breakpoints, script list, method outline, status bar

15. `FileSystem dock parity: browser, file ops, and resource drag-drop integration`
    Source: file browser, move/rename/delete, drag to inspector/scene tree, previews, import hooks

16. `Signals dock parity: signal browsing, connection dialog, and connection management`
    Source: Node dock signals/groups tabs, signal connection UI, disconnect/navigation workflows

17. `Animation editor parity: AnimationPlayer, timeline, tracks, and AnimationTree`
    Source: animation panel, timeline, keyframes, track types, bezier, onion skinning, AnimationTree

18. `Editor systems parity: project settings, editor settings, VCS, export, and variant coverage`
    Source: project/editor settings dialogs, VCS/export surfaces, Appendix A variant coverage expectations

## Notes

- The gizmo gap belongs primarily to:
  - `Viewport parity: transform gizmos and pivot workflows`
  - with some overlap into `Viewport parity: selection modes, zoom/pan, and viewport controls`

- The scene tree and inspector sync problems belong primarily to:
  - `Scene Tree parity: indicators, badges, and selection state`
  - `Inspector parity: core property editing and interaction`

- Do not let these beads outrank runtime/oracle parity work until `pat-4ap` is complete.
