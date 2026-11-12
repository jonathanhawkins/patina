# Editor Parity — Inspector core property editing and interaction

Lane source: `prd/EDITOR_PARITY_BEADS.md` lane 4 — "Inspector parity: core
property editing and interaction" (property editors, drag-to-adjust,
revert/defaults, linked values, copy/paste paths).

This execution map enumerates the concrete beads required for Inspector core
property-editing parity with Godot: per-type property editors and commit
semantics, numeric drag-to-adjust and inline expression entry, revert-to-
default, proportional/linked vector components, value and property-path
copy/paste, multi-node edit, animation keying from the inspector, and
undo/redo + dirty integration.

## Format

Each bead is listed as `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test the planner wires into criteria-driven
analysis: `(test: \`<test_name>\`)`.

## Now

1. `inspector-props-typed-editors` Property editors render and commit per type (numeric, string, bool, enum, Vector2/3, Color, NodePath, resource)
   Acceptance: Each supported property type renders its matching editor widget and committing an edit writes the typed value back to the object (test: `inspector_typed_property_editors`)

2. `inspector-props-drag-to-adjust` Numeric fields support click-drag scrubbing to adjust the value with step granularity
   Acceptance: Dragging horizontally on a numeric editor changes the value by the field's step, respects min/max range, and commits on release (test: `inspector_numeric_drag_to_adjust`)

3. `inspector-props-inline-expression` Numeric fields accept inline math expressions that evaluate on commit
   Acceptance: Entering an expression like `2*PI` or `1+1` into a numeric field evaluates to the computed number on commit, and an invalid expression is rejected without mutating the value (test: `inspector_numeric_inline_expression`)

4. `inspector-props-revert-default` A revert button appears when a property differs from its default and resets it on click
   Acceptance: A property whose value differs from the object default shows a revert affordance that, when activated, restores the default value and clears the override (test: `inspector_revert_to_default`)

5. `inspector-props-linked-components` Vector/Rect editors offer a proportional lock so editing one component scales the others by ratio
   Acceptance: With the proportional lock enabled, editing one component of a multi-component property scales the remaining components by the original ratio; with it disabled, only the edited component changes (test: `inspector_linked_proportional_components`)

6. `inspector-props-copy-paste-value` Copy/Paste property value transfers a value between compatible properties
   Acceptance: Copying a property value and pasting it onto a type-compatible property writes the value, and pasting onto an incompatible type is rejected (test: `inspector_copy_paste_property_value`)

7. `inspector-props-copy-property-path` Copy Property Path yields the scripting path for the selected property
   Acceptance: The copy-property-path action produces the property's resolvable path string (e.g. `position:x`) for the selected sub-property (test: `inspector_copy_property_path`)

8. `inspector-props-multi-edit` Editing a property with multiple objects selected applies the change to all of them
   Acceptance: With several nodes selected, committing a shared property applies the new value to every selected node in a single undo step (test: `inspector_multi_node_property_edit`)

9. `inspector-props-key-animation` A key affordance lets a property value be inserted as an animation track keyframe from the inspector
   Acceptance: When an AnimationPlayer track context is active, the inspector key affordance inserts a keyframe for the property at the current time with the editor's value (test: `inspector_key_property_to_animation`)

10. `inspector-props-undo-redo-dirty` Property edits push a reversible undo/redo entry and mark the scene dirty
    Acceptance: Committing a property change records an undo entry that restores the prior value on undo and reapplies it on redo, and the owning scene is marked modified (test: `inspector_property_undo_redo_dirty`)
