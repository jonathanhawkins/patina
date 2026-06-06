# Editor Parity — Inspector advanced property organization and exported script fields

Per-lane execution map for the Inspector's advanced organization features and
script-exported property handling. Source lane: `Inspector parity: advanced
property organization and exported script fields` in
`prd/EDITOR_PARITY_BEADS.md` (lane 5). Source surface: favorites, grouped
exports, sub-resource inline editing, hints/categories.

Scope: how properties are *organized and surfaced* — class categories, export
groups/subgroups, favorites/pinning, inline sub-resource expansion, and the
PropertyHint → editor-widget mapping that `@export` annotations drive. Basic
per-type editing and the toolbar are out of scope (see
`EDITOR_PARITY_INSPECTOR_PROPERTIES_MAP.md` and
`EDITOR_PARITY_INSPECTOR_TOOLBAR_MAP.md`).

## Format

Each bead is `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test in the form `(test: \`<test_name>\`)` the
planner wires into criteria-driven analysis.

## Now

1. `inspector-property-categories` Render class-based category separators (with class icon and name) that group inherited properties by the class that declares them, matching Godot's EditorInspector category rows.
   Acceptance: inspecting a node shows category headers for each declaring class in inheritance order with the class icon (test: `inspector_property_categories_group_by_class`)

2. `inspector-export-groups` Implement `@export_group` / `@export_subgroup` collapsing so prefixed properties nest under collapsible group and subgroup headers.
   Acceptance: a script with an export group and subgroup renders nested collapsible headers and toggling a group hides its members (test: `inspector_export_groups_nest_and_collapse`)

3. `inspector-export-hint-widgets` Map `PropertyHint` values from `@export` annotations to the correct editor widget (range slider, enum dropdown, file/dir picker, multiline text, flags).
   Acceptance: exported fields with range, enum, file, and multiline hints each render their hint-specific widget rather than the default editor (test: `inspector_export_hints_select_widget`)

4. `inspector-subresource-inline-edit` Allow a resource-typed property to expand inline so its sub-properties are editable without leaving the parent object.
   Acceptance: expanding an embedded resource property shows its editable sub-properties inline and edits persist to the sub-resource (test: `inspector_subresource_inline_edit_persists`)

## Next

5. `inspector-favorites-pinning` Implement property favorites so a user can pin properties to a Favorites section that stays at the top across objects of the same class.
   Acceptance: favoriting a property moves it into a top Favorites section and re-inspecting a same-class object keeps it favorited (test: `inspector_favorites_pin_to_top`)

6. `inspector-export-category-separator` Support `@export_category` to insert a labeled category divider in script-declared property order.
   Acceptance: a script using `@export_category` renders a labeled divider at the declared position separating the following exports (test: `inspector_export_category_divider_renders`)

7. `inspector-typed-collection-export` Render typed array and dictionary exports with add/remove/reorder controls and per-element editors driven by the element type hint.
   Acceptance: a typed array export shows add/remove/reorder controls and elements use the element-type editor; a typed dictionary edits keys and values (test: `inspector_typed_collection_export_edits`)

## Later

8. `inspector-usage-flags-visibility` Honor property usage flags (storage, editor, read-only, no-editor) so script and engine properties show, hide, or lock in the inspector accordingly.
   Acceptance: a read-only-flagged export renders disabled, a no-editor-flagged property is hidden, and a storage-only property does not appear (test: `inspector_usage_flags_control_visibility`)
