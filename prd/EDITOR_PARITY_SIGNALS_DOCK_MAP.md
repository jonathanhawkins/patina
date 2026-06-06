# Editor Parity — Signals dock browsing, connection dialog, and connection management

Per-lane execution map for the Node dock's Signals tab. Source lane: `Signals
dock parity: signal browsing, connection dialog, and connection management` in
`prd/EDITOR_PARITY_BEADS.md` (lane 16). Source surface: Node dock
signals/groups tabs, signal connection UI, disconnect/navigation workflows.

Scope: the signal tree, the Connect-a-Signal dialog (target node/method,
binds, flags), and managing existing connections (edit, disconnect, navigate,
auto-generate receiver). The Groups tab is a sibling concern; this lane is the
signal-connection workflow.

## Format

Each bead is `` N. `key-slug` Description `` followed by an
`Acceptance:` line naming the test in the form `(test: \`<test_name>\`)` the
planner wires into criteria-driven analysis.

## Now

1. `signals-dock-tree` Render the Signals tab as a tree of the selected node's signals grouped by declaring class, with signal signatures and existing connections nested under each signal.
   Acceptance: selecting a node lists its signals grouped by class with signatures, and signals with connections show them as children (test: `signals_dock_tree_lists_signals_and_connections`)

2. `signals-connect-dialog` Implement the Connect-a-Signal dialog to pick a target node and target method from the scene, creating the connection on confirm.
   Acceptance: connecting a signal to a target node+method via the dialog creates a persisted connection shown under the signal (test: `signals_connect_dialog_creates_connection`)

3. `signals-connection-flags-binds` Support the dialog's advanced options — extra bound arguments and the deferred/one-shot connect flags — and persist them on the connection.
   Acceptance: a connection made with bound args and deferred/one-shot flags persists those settings and they are visible when editing the connection (test: `signals_connection_flags_and_binds_persist`)

4. `signals-disconnect-edit` Support editing and disconnecting an existing connection from the signal tree.
   Acceptance: editing a connection updates its target/binds/flags and disconnect removes it from the node and the tree (test: `signals_disconnect_and_edit_connection`)

## Next

5. `signals-navigate-to-method` Implement go-to-method navigation that opens the target script at the connected receiver method.
   Acceptance: activating a connection navigates the script editor to the receiver method definition (test: `signals_navigate_to_connected_method`)

6. `signals-autocreate-receiver` Auto-generate a receiver method stub in the target node's script when connecting to a method that does not yet exist.
   Acceptance: confirming a connection to a non-existent method appends a correctly-signatured method stub to the target script (test: `signals_autocreate_receiver_method_stub`)

## Later

7. `signals-docs-tooltips` Surface signal documentation/descriptions as tooltips and a details area in the signal tree.
   Acceptance: hovering or selecting a signal shows its documentation text sourced from the class reference (test: `signals_docs_tooltips_render`)

8. `signals-filter-search` Implement a filter box that narrows the signal tree to name matches.
   Acceptance: typing in the filter narrows the signal tree to matching signals and clearing restores the full tree (test: `signals_filter_search_narrows_tree`)
