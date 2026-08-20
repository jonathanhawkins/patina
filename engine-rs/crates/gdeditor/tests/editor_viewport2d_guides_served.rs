//! pat-s3rrv.149 — the served 2D viewport can reach its guides and Frame
//! Selection endpoints.
//!
//! Godot ground truth: dragging out of a ruler drops a guide that snapping
//! honours (`editor/scene/canvas_item_editor_plugin.cpp`,
//! `_edit_horizontal_guides_` / `_edit_vertical_guides_`), and
//! `VIEW_FRAME_SELECTION` (F) recentres and zooms onto the selection.
//!
//! ⚠️ **Both halves already worked server-side and neither was reachable.**
//! `GET`/`POST /api/viewport/guides` are dispatched and the stored guides are
//! already consumed by `apply_snap`; `POST /api/viewport/frame_selection` has
//! four passing server tests. Neither string occurred anywhere in
//! `EDITOR_HTML` — the client drew rulers and fetched only
//! `/api/viewport/snap_guides`, which is transient snap FEEDBACK, not user
//! guides. This file pins the client side, which is the half that was missing.

use gdeditor::editor_ui::EDITOR_HTML;

/// A window of the client source following the first occurrence of `needle`,
/// so an assertion can check the CONTEXT a URL is used in rather than merely
/// that the string exists somewhere in 600KB of JavaScript.
fn window_after(needle: &str, len: usize) -> String {
    let start = EDITOR_HTML
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` must appear in EDITOR_HTML"));
    EDITOR_HTML[start..].chars().take(len).collect()
}

/// 🔑 `/api/viewport/guides` is NOT a substring of `/api/viewport/snap_guides`.
///
/// ⚠️ This is the trap the whole file turns on. The client already fetched
/// `/api/viewport/snap_guides`, so a careless `contains("guides")` — or even
/// `contains("viewport/guides")` had the paths been nested differently — would
/// have reported this bead as already done. Asserting the distinction
/// explicitly means the guard cannot be satisfied by the pre-existing
/// transient-feedback call.
#[test]
fn the_guides_path_is_not_satisfied_by_the_snap_guides_path() {
    assert!(
        !"/api/viewport/snap_guides".contains("/api/viewport/guides"),
        "🔑 if these two paths ever overlap, every assertion below becomes \
         vacuous — the pre-existing snap-feedback fetch would satisfy them"
    );
    assert!(
        EDITOR_HTML.contains("/api/viewport/snap_guides"),
        "the transient snap-feedback fetch must still be there — this bead adds \
         user guides beside it, it does not replace it"
    );
}

/// The client both READS and WRITES the guide list.
///
/// ⚠️ Two separate assertions on purpose. A client that only POSTed would
/// create guides it never draws; one that only GETed could never make one. The
/// bead's failure mode was neither call existing at all.
#[test]
fn the_client_reads_and_writes_viewport_guides() {
    assert!(
        EDITOR_HTML.contains("/api/viewport/guides"),
        "the client must reference the guides endpoint at all"
    );

    let read = window_after("api('GET', '/api/viewport/guides')", 400);
    assert!(
        read.contains("viewportGuides"),
        "the GET must store its result where the overlay draw pass can read it; \
         got: {read}"
    );

    let write = window_after("api('POST', '/api/viewport/guides'", 400);
    assert!(
        write.contains("orientation") && write.contains("position"),
        "the POST must send Godot's guide shape — an orientation and a \
         position, which is what api_post_guides parses; got: {write}"
    );
}

/// The guides are drawn, not merely fetched.
///
/// ⚠️ A fetch that lands in a variable nothing renders is the same invisible
/// result as no fetch at all — and it would still satisfy a
/// `contains("/api/viewport/guides")` check.
#[test]
fn the_fetched_guides_reach_the_overlay_draw_pass() {
    assert!(
        EDITOR_HTML.contains("viewportGuides.slice()"),
        "the overlay pass must read the stored guide list"
    );
    assert!(
        EDITOR_HTML.contains("pendingGuide"),
        "a guide being dragged out of a ruler must be drawn before it is \
         committed, or the drag has no feedback"
    );
}

/// Frame Selection is bound for 2D, and does not collide with the 3D binding.
///
/// ⚠️ `F` was already bound — to `/api/viewport/3d/frame`, guarded on
/// `editorMode === '3d'`. The 2D binding has to be a separate branch, and it
/// must not fire on Ctrl/Cmd-F, which is the script find shortcut.
#[test]
fn frame_selection_is_bound_in_two_d_without_stealing_the_other_f_bindings() {
    assert!(
        EDITOR_HTML.contains("/api/viewport/frame_selection"),
        "the 2D Frame Selection endpoint must be reachable from the client"
    );
    let bind = window_after("api('POST', '/api/viewport/frame_selection'", 500);
    for applied in ["zoom", "pan_x", "pan_y"] {
        assert!(
            bind.contains(applied),
            "the client must adopt `{applied}` from the response — the server \
             does the fitting and the client only applies it; got: {bind}"
        );
    }

    // The 3D binding must survive untouched.
    assert!(
        EDITOR_HTML.contains("/api/viewport/3d/frame"),
        "F in 3D still frames through the 3D endpoint"
    );
}

/// An existing guide can be moved, and dropped back on a ruler to delete it.
///
/// ⚠️ The server exposes create and `{"remove": i}` but **no move**, so the
/// client has to compose one out of the two. That is worth pinning: a client
/// that only ever created would leave the old line behind on every drag, which
/// looks like the guide duplicating rather than moving.
#[test]
fn an_existing_guide_can_be_dragged_and_deleted() {
    assert!(
        EDITOR_HTML.contains("draggingGuide"),
        "the client must track which placed guide is under the cursor"
    );
    assert!(
        EDITOR_HTML.contains("{ remove: moved.index }"),
        "deleting must go through the server's index-based remove"
    );
    assert!(
        EDITOR_HTML.contains("guidesToDraw[draggingGuide.index] = null"),
        "the copy at the OLD position must be suppressed while dragging, or the \
         line appears to duplicate and releasing makes one of the two vanish"
    );
}

/// 🔑 A move is remove-THEN-create, in that order.
///
/// ⚠️ The server removes **by index**, and the index was captured when the drag
/// began. Creating first would insert an entry ahead of that removal and delete
/// the wrong guide — a bug that only shows up with two or more guides placed,
/// which is exactly the case a single-guide test would miss.
#[test]
fn a_guide_move_removes_before_it_recreates() {
    let start = EDITOR_HTML
        .find("{ remove: moved.index }")
        .expect("the remove call must exist");
    let after: String = EDITOR_HTML[start..].chars().take(400).collect();
    assert!(
        after.contains("orientation") && after.contains("position"),
        "🔑 the re-create must follow the remove in the same chain; got: {after}"
    );
    // …and the create must not appear BEFORE it. Built by reversing twice so
    // the window is the last 400 CHARS (not bytes) before the remove, then read
    // in normal order — a raw byte slice could split a multi-byte character.
    let before: String = EDITOR_HTML[..start]
        .chars()
        .rev()
        .take(400)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    assert!(
        !before.contains("orientation"),
        "🔑 a create immediately preceding the remove would mean the captured \
         index refers to a stale list and the wrong guide gets deleted; \
         got: {before}"
    );
}

/// Grabbing empty viewport space must still reach node selection.
///
/// ⚠️ The guide hit-test runs on the same container mousedown that node
/// picking depends on. It may only call `preventDefault`/`stopPropagation`
/// when a guide is genuinely under the cursor — an unconditional swallow would
/// make the whole 2D viewport unclickable, which no guide test would notice.
#[test]
fn the_guide_grab_only_swallows_the_event_on_a_hit() {
    let start = EDITOR_HTML
        .find("var GRAB = 4;")
        .expect("the grab-slop constant marks the hit-test");
    let hit: String = EDITOR_HTML[start..].chars().take(600).collect();
    let prevent = hit.find("e.preventDefault()");
    let if_hit = hit.find("if (hit)");
    assert!(
        prevent.is_some() && if_hit.is_some() && if_hit < prevent,
        "🔑 preventDefault must sit INSIDE the `if (hit)` branch; got: {hit}"
    );
}
