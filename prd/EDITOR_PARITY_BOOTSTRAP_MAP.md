# Editor Parity Bootstrap — author the per-lane execution maps

This is a meta-execution-map: each bead asks a worker to author one new
execution map describing a slice of the editor parity backlog. The lane
inventory comes from `prd/EDITOR_PARITY_BEADS.md` (18 lanes), with one
additional aggregator at the end.

When all beads here close, `prd/EDITOR_PARITY_EXECUTION_MAP.md` and
`prd/EDITOR_PARITY_EXIT.md` exist and the planner advances to the
editor-parity phase, which then seeds lane-level beads.

## Format every authored map must follow

Each new file must:

1. Begin with a `## Now` / `## Next` / `## Later` priority section.
2. List beads as: `` N. `key-slug` Description ``
3. Follow each bead with: `   Acceptance: <single-sentence proof, including the test name>`
4. The acceptance line must contain a test name in the form `(test: \`<test_name>\`)` so the planner can wire it into criteria-driven analysis.

A worker is "done" with a meta-bead when the named file exists, parses
correctly with `patina-orchestrator plan --dry-run`, and the corresponding
bootstrap test passes (the test merely asserts the file exists and has at
least the minimum item count).

## Now

1. `editor-parity-author-scene-tree-ops-map` Author execution map for Scene Tree parity (node operations and hierarchy)
   Acceptance: prd/EDITOR_PARITY_SCENE_TREE_OPS_MAP.md exists with at least 8 numbered items each having an Acceptance line (test: `bootstrap_scene_tree_ops_map_authored`)

2. `editor-parity-author-scene-tree-indicators-map` Author execution map for Scene Tree indicators, badges, and selection state
   Acceptance: prd/EDITOR_PARITY_SCENE_TREE_INDICATORS_MAP.md exists with at least 6 numbered items (test: `bootstrap_scene_tree_indicators_map_authored`)

3. `editor-parity-author-inspector-toolbar-map` Author execution map for Inspector resource toolbar, history, and object navigation
   Acceptance: prd/EDITOR_PARITY_INSPECTOR_TOOLBAR_MAP.md exists with at least 6 numbered items (test: `bootstrap_inspector_toolbar_map_authored`)

4. `editor-parity-author-inspector-properties-map` Author execution map for Inspector core property editing and interaction
   Acceptance: prd/EDITOR_PARITY_INSPECTOR_PROPERTIES_MAP.md exists with at least 8 numbered items (test: `bootstrap_inspector_properties_map_authored`)

5. `editor-parity-author-inspector-advanced-map` Author execution map for Inspector advanced property organization and exported script fields
   Acceptance: prd/EDITOR_PARITY_INSPECTOR_ADVANCED_MAP.md exists with at least 6 numbered items (test: `bootstrap_inspector_advanced_map_authored`)

6. `editor-parity-author-viewport-selection-map` Author execution map for Viewport selection modes, zoom/pan, and viewport controls
   Acceptance: prd/EDITOR_PARITY_VIEWPORT_SELECTION_MAP.md exists with at least 6 numbered items (test: `bootstrap_viewport_selection_map_authored`)

7. `editor-parity-author-viewport-gizmos-map` Author execution map for Viewport transform gizmos and pivot workflows
   Acceptance: prd/EDITOR_PARITY_VIEWPORT_GIZMOS_MAP.md exists with at least 6 numbered items (test: `bootstrap_viewport_gizmos_map_authored`)

8. `editor-parity-author-viewport-overlays-map` Author execution map for Viewport snapping, guides, rulers, grid, and canvas overlays
   Acceptance: prd/EDITOR_PARITY_VIEWPORT_OVERLAYS_MAP.md exists with at least 6 numbered items (test: `bootstrap_viewport_overlays_map_authored`)

9. `editor-parity-author-top-bar-map` Author execution map for Top bar scene tabs, run controls, and editor mode switching
   Acceptance: prd/EDITOR_PARITY_TOP_BAR_MAP.md exists with at least 6 numbered items (test: `bootstrap_top_bar_map_authored`)

10. `editor-parity-author-menus-map` Author execution map for Menu scene/project/debug/editor/help actions
    Acceptance: prd/EDITOR_PARITY_MENUS_MAP.md exists with at least 6 numbered items (test: `bootstrap_menus_map_authored`)

11. `editor-parity-author-create-node-map` Author execution map for Create Node dialog parity for 2D workflows
    Acceptance: prd/EDITOR_PARITY_CREATE_NODE_MAP.md exists with at least 6 numbered items (test: `bootstrap_create_node_map_authored`)

12. `editor-parity-author-bottom-panels-map` Author execution map for Bottom panels parity (output, debugger, monitors, audio buses, shader editor)
    Acceptance: prd/EDITOR_PARITY_BOTTOM_PANELS_MAP.md exists with at least 6 numbered items (test: `bootstrap_bottom_panels_map_authored`)

13. `editor-parity-author-script-editor-core-map` Author execution map for Script editor core editing features
    Acceptance: prd/EDITOR_PARITY_SCRIPT_EDITOR_CORE_MAP.md exists with at least 8 numbered items (test: `bootstrap_script_editor_core_map_authored`)

14. `editor-parity-author-script-editor-nav-map` Author execution map for Script editor search, navigation, debugging, and script panel
    Acceptance: prd/EDITOR_PARITY_SCRIPT_EDITOR_NAV_MAP.md exists with at least 6 numbered items (test: `bootstrap_script_editor_nav_map_authored`)

15. `editor-parity-author-filesystem-dock-map` Author execution map for FileSystem dock browser, file ops, and resource drag-drop integration
    Acceptance: prd/EDITOR_PARITY_FILESYSTEM_DOCK_MAP.md exists with at least 6 numbered items (test: `bootstrap_filesystem_dock_map_authored`)

16. `editor-parity-author-signals-dock-map` Author execution map for Signals dock signal browsing, connection dialog, and connection management
    Acceptance: prd/EDITOR_PARITY_SIGNALS_DOCK_MAP.md exists with at least 6 numbered items (test: `bootstrap_signals_dock_map_authored`)

17. `editor-parity-author-animation-editor-map` Author execution map for Animation editor (AnimationPlayer, timeline, tracks, AnimationTree)
    Acceptance: prd/EDITOR_PARITY_ANIMATION_EDITOR_MAP.md exists with at least 8 numbered items (test: `bootstrap_animation_editor_map_authored`)

18. `editor-parity-author-editor-systems-map` Author execution map for Editor systems (project settings, editor settings, VCS, export, variant coverage)
    Acceptance: prd/EDITOR_PARITY_EDITOR_SYSTEMS_MAP.md exists with at least 6 numbered items (test: `bootstrap_editor_systems_map_authored`)

## Next

19. `editor-parity-author-aggregate-map` Aggregate the per-lane maps into the canonical editor-parity execution map and exit criteria
    Acceptance: prd/EDITOR_PARITY_EXECUTION_MAP.md exists and concatenates every per-lane map (preserving Now/Next/Later ordering); prd/EDITOR_PARITY_EXIT.md exists with one unchecked checkbox per acceptance test referenced from the maps (test: `bootstrap_aggregate_editor_parity_map_assembled`)
