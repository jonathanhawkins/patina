# Editor Parity — Scene Tree node operations and hierarchy workflows

Lane source: `prd/EDITOR_PARITY_BEADS.md` lane 1 — "Scene Tree parity: node
operations and hierarchy workflows" (Scene Tree dock node operations,
hierarchy features, context menu, groups).

This execution map enumerates the concrete node-operation and hierarchy beads
required for Scene Tree dock parity with Godot. It covers adding/instancing
nodes, the full context-menu action set, reparenting and ordering within the
hierarchy, clipboard operations, type changes, ownership (local/editable
children), and group membership editing.

## Format

Each bead is listed as `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test the planner wires into criteria-driven
analysis: `(test: \`<test_name>\`)`.

## Now

1. `scene-tree-ops-add-child-node` Add Child Node action inserts a new node of the chosen type as a child of the selected node and selects it
   Acceptance: Adding a child node to a selected parent produces a child of the requested type, focuses it in the tree, and marks the scene dirty (test: `scene_tree_add_child_node`)

2. `scene-tree-ops-instance-child-scene` Instance Child Scene action embeds an external scene as an instanced child with the instance badge
   Acceptance: Instancing a `.tscn` as a child creates an instanced node rooted at the selected parent and records its source path for the instance indicator (test: `scene_tree_instance_child_scene`)

3. `scene-tree-ops-rename-node` Rename via double-click / F2 updates the node name with collision-safe uniquification among siblings
   Acceptance: Renaming a node commits the new name, rejects empty names, and auto-suffixes a duplicate sibling name to keep names unique (test: `scene_tree_rename_node_unique`)

4. `scene-tree-ops-delete-node` Delete (Del) removes the selected node(s) and their subtrees, with multi-selection support
   Acceptance: Deleting one or more selected nodes removes their full subtrees from the scene and clears the selection without orphaning descendants (test: `scene_tree_delete_node_subtree`)

5. `scene-tree-ops-duplicate-node` Duplicate (Ctrl+D) clones the selected node and its subtree as a sibling with a uniquified name
   Acceptance: Duplicating a node produces a deep copy of its subtree inserted as the next sibling, with a unique name and copied properties (test: `scene_tree_duplicate_node`)

6. `scene-tree-ops-reparent-node` Reparent (drag-drop or Reparent dialog) moves a node under a new parent while preserving its subtree
   Acceptance: Reparenting a node moves it and its descendants under the target parent, rejects reparenting a node into its own descendant, and preserves global transform when requested (test: `scene_tree_reparent_node`)

7. `scene-tree-ops-reorder-siblings` Move Up / Move Down reorders a node among its siblings
   Acceptance: Move Up / Move Down changes the node's index among siblings, is a no-op at the boundaries, and updates child ordering deterministically (test: `scene_tree_reorder_siblings`)

8. `scene-tree-ops-cut-copy-paste` Cut / Copy / Paste node clipboard operations move or clone subtrees across the hierarchy
   Acceptance: Copy then Paste inserts a clone of the copied subtree under the paste target, Cut then Paste moves the original, and paste uniquifies names (test: `scene_tree_cut_copy_paste_node`)

9. `scene-tree-ops-change-type` Change Type converts a node to a compatible type, preserving children and shared properties
   Acceptance: Changing a node's type replaces it with the chosen type in place, retains its children and name, and carries over properties common to both types (test: `scene_tree_change_node_type`)

10. `scene-tree-ops-editable-children` Editable Children / Make Local toggles ownership and exposes instanced subtree nodes for editing
    Acceptance: Toggling Editable Children reveals an instance's internal nodes as editable, and Make Local converts an instanced node into an owned local subtree (test: `scene_tree_editable_children_make_local`)

11. `scene-tree-ops-group-membership` Context-menu group editing adds and removes the selected node from named groups
    Acceptance: Adding a node to a group records the membership and the group badge, and removing it clears the membership; group lists stay consistent across nodes (test: `scene_tree_group_membership`)
