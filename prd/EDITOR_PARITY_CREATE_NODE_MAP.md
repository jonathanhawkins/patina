# Editor Parity — Create Node dialog for 2D workflows

Lane source: `prd/EDITOR_PARITY_BEADS.md` lane 11 — "Create Node dialog parity
for 2D workflows" (searchable node dialog, favorites/recent, 2D node catalog
and common helper nodes).

This execution map enumerates the concrete beads required for Create Node
dialog parity with Godot: the inheritance node-type tree, incremental search
and best-match confirmation, favorites and recent sections, the per-type
description/help panel, instantiation as a child of the current selection,
and 2D node catalog coverage.

## Format

Each bead is listed as `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test the planner wires into criteria-driven
analysis: `(test: \`<test_name>\`)`.

## Now

1. `create-node-type-tree` The dialog shows the node-type inheritance tree rooted at the requested base type
   Acceptance: Opening Create Node renders the class tree rooted at the base type (e.g. Node) with inheritance nesting, expandable to leaf types (test: `create_node_type_tree`)

2. `create-node-incremental-search` Typing in the search box incrementally filters the tree and highlights matches
   Acceptance: Entering search text filters the tree to matching type names (substring/fuzzy), keeps ancestors visible for context, and highlights the matched span (test: `create_node_incremental_search`)

3. `create-node-best-match-confirm` Enter confirms the best-matching type and double-click / Create button instantiates the selected type
   Acceptance: Pressing Enter selects the top-ranked search match, and double-clicking a type or activating Create instantiates the selected type and closes the dialog (test: `create_node_best_match_confirm`)

4. `create-node-favorites` Types can be favorited and appear in a Favorites section at the top of the dialog
   Acceptance: Toggling favorite on a type adds it to the persistent Favorites section, toggling off removes it, and favorites survive reopening the dialog (test: `create_node_favorites`)

5. `create-node-recent` Recently created types appear in a Recent section ordered by last use
   Acceptance: Creating a node records its type in the Recent section, ordered most-recent-first and capped to a recent-history limit (test: `create_node_recent`)

6. `create-node-description-panel` Selecting a type shows its description/help text in the dialog
   Acceptance: Selecting a type populates a description panel with the type's summary/help text, and unknown/empty descriptions render a graceful placeholder (test: `create_node_description_panel`)

7. `create-node-child-of-selection` The created node is inserted as a child of the currently selected scene node (or as root when empty)
   Acceptance: Confirming creation inserts the new node as a child of the active selection, focuses it in the scene tree, and makes it the scene root when the scene is empty (test: `create_node_child_of_selection`)

8. `create-node-2d-catalog` The dialog exposes the common 2D node catalog and helper nodes
   Acceptance: The type tree includes the core 2D nodes (Node2D, Sprite2D, AnimatedSprite2D, CollisionShape2D, Area2D, CharacterBody2D, Camera2D) and they instantiate correctly (test: `create_node_2d_catalog`)
