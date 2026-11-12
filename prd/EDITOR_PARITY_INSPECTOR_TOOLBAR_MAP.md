# Editor Parity — Inspector resource toolbar, history, and object navigation

Per-lane execution map for the Inspector top toolbar, header, back/forward
history, and sub-resource navigation. Source lane: `Inspector parity:
resource toolbar, history, and object navigation` in
`prd/EDITOR_PARITY_BEADS.md` (lane 3).

Scope: the controls that sit *above* the property list — the object header
(class icon + name), back/forward navigation, the inspected-object history
dropdown, the resource action buttons (edit/load/save/clear/copy/paste), the
sub-resource breadcrumb, and the pin/lock toggle. Core property editing is
out of scope (see `EDITOR_PARITY_INSPECTOR_PROPERTIES_MAP.md`).

## Format

Each bead is `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test the planner wires into criteria-driven
analysis, in the form `(test: \`<test_name>\`)`.

## Now

1. `inspector-object-header` Render the inspected-object header with class icon, object/resource name, and class label matching Godot's EditorInspector header row.
   Acceptance: inspecting a node shows a header line with its class name and resource path; clearing the inspector hides it (test: `inspector_object_header_renders_class_and_name`)

2. `inspector-history-back-forward` Implement back/forward navigation buttons that move through the stack of previously inspected objects, enabling/disabling at the ends of the stack.
   Acceptance: inspecting object A then B then pressing Back re-selects A and enables Forward; Back at the start is disabled (test: `inspector_history_back_forward_navigates_stack`)

3. `inspector-history-dropdown` Add the history dropdown menu listing recently inspected objects, selecting an entry re-inspects that object.
   Acceptance: after inspecting three objects the dropdown lists all three most-recent-first and selecting one inspects it (test: `inspector_history_dropdown_lists_and_selects`)

4. `inspector-resource-toolbar-actions` Add the resource action toolbar (edit, load, clear, save-as, copy, paste) wired to the inspected Resource, disabling actions that do not apply to the current object.
   Acceptance: inspecting a Resource exposes load/clear/copy/paste actions and clear empties the slot; inspecting a plain Node hides resource-only actions (test: `inspector_resource_toolbar_actions_apply`)

## Next

5. `inspector-subresource-breadcrumb` Render a sub-resource breadcrumb so editing an embedded resource shows the path back to the owning object and clicking a crumb navigates up.
   Acceptance: editing a sub-resource of a node shows a breadcrumb with the parent object and clicking the parent crumb re-inspects it (test: `inspector_subresource_breadcrumb_navigates_up`)

6. `inspector-pin-lock` Implement the pin/lock toggle that keeps the current object inspected even as the scene-tree selection changes.
   Acceptance: pinning the inspector keeps object A shown after selecting object B in the scene tree; unpinning resumes following selection (test: `inspector_pin_lock_holds_selection`)

7. `inspector-copy-paste-resource` Implement copy/paste of a resource reference across the resource toolbar so a copied resource can be pasted into a compatible property slot.
   Acceptance: copying a resource from one object and pasting into a compatible slot on another assigns the same resource; incompatible slots reject the paste (test: `inspector_copy_paste_resource_round_trip`)

## Later

8. `inspector-history-persists-per-scene` Persist the inspected-object history per open scene so switching scene tabs restores that scene's navigation stack.
   Acceptance: building history in scene A, switching to scene B, then back to A restores A's back/forward stack (test: `inspector_history_persists_per_scene`)
