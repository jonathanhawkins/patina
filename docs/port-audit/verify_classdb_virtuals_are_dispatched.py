#!/usr/bin/env python3
"""Guard: a ClassDB-registered script virtual must be INVOKED by the engine.

pat-nzgdu's systemic finding is "real tested code UNWIRED from the runtime
(64 UNWIRED + 16 STUB)". This turns that one-off audit into a repeatable check,
because the failure mode is silent and grows: registering a virtual in ClassDB
makes `ClassDB.class_has_method` answer true and lets a GDScript override it,
while the engine never calls it — so the override runs never and the script sees
no error. class_db.rs's own comments warn about the same shape for non-virtual
methods ("ClassDB may not advertise what the engine cannot run").

MEASURED STATE when this guard was written (2026-10-30):
    38 non-Extension classes register at least one `_`-prefixed virtual
    214 distinct script-overridable virtuals
    156 of them (73%) have ZERO call sites outside the registration table
    246 (class, virtual) pairs; 181 unwired under per-name attribution, and
        190 once the 9 confirmed false credits below are added back — so the
        per-CLASS surface is 190 of 246 (77%), worse than the name figure

SCOPE, deliberately narrow so the result means something:
  * `*Extension` classes are EXCLUDED. Their virtuals (PhysicsServer2DExtension
    `_area_create`, …) are GDExtension-only; a GDScript cannot override a
    pointer-typed virtual at all, so "never called from Rust" is not a finding
    there. Including them would add ~670 names of pure noise.
  * The unit is the virtual NAME. A name is either invoked somewhere or it is
    not. **This makes the 156 EXACT, not a floor** — sharing a name between
    classes cannot change whether that name has a call site. See
    SHARED_OWNERSHIP_FALSE_CREDITS for what the name unit does cost, which is
    per-CLASS attribution, and for why that is a much narrower blind spot than
    the companion methods guard's (whose unit is the class, giving it a 91-vs-179
    band where this one has none).

WHY A QUOTED-LITERAL SCAN IS SOUND HERE, checked rather than assumed:
  * Positives work: `_process_custom_fx` resolves to control_render.rs and
    `_get_code` to visual_shader_custom.rs, the two virtuals known to be wired.
  * Negatives were hand-verified. `_can_drop_data`'s only other mentions are DOC
    COMMENTS in gdplatform/src/drag_drop.rs ("Mirrors Godot's
    `_can_drop_data()`") — the Rust side implements drag-and-drop natively and
    never consults a script override. `_initialize`/`_finalize` have zero
    references of any kind.
  * No virtual is dispatched through a CONSTRUCTED name: there is no
    `call_script_method(.., &format!(..))` anywhere, so every dispatch is a
    literal this scan can see.
  * Generated metadata tables (inspector_schema_generated.rs,
    create_node_catalog.rs, create_dialog.rs) are excluded alongside
    class_db.rs: they list method names without calling them, so counting them
    would mark every virtual "wired".

PINNED IN BOTH DIRECTIONS:
  * A NEW unwired virtual fails -> you registered a virtual without wiring it.
  * A baselined virtual that becomes wired ALSO fails -> shrink the list. That
    is the intended direction of travel and keeps the number honest.

Usage:
    python3 docs/port-audit/verify_classdb_virtuals_are_dispatched.py
    python3 docs/port-audit/verify_classdb_virtuals_are_dispatched.py --report
    python3 docs/port-audit/verify_classdb_virtuals_are_dispatched.py --self-test
"""
from __future__ import annotations

import glob
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ENGINE = os.path.join(REPO, "engine-rs")
CLASS_DB = os.path.join(ENGINE, "crates/gdobject/src/class_db.rs")

# Files that NAME methods without calling them. Counting these would make every
# virtual look wired, which is the one mistake that would make this guard
# useless rather than merely wrong.
NON_CALLING = (
    "class_db.rs",
    "inspector_schema_generated.rs",
    "create_node_catalog.rs",
    "create_dialog.rs",
)

# A virtual registered on SEVERAL classes is "wired" to this guard as soon as ANY
# one of its owners is served, because the unit is the name. That is sound for
# the name count and WRONG for per-class attribution, so the cost is measured
# here instead of being left as a worry.
#
# 27 of the 214 virtuals have more than one non-Extension owner; 6 of those 27
# are wired. All 6 were checked by hand against their single call site, and the
# nine (class, virtual) pairs below are credited by a SIBLING's call site alone.
# Each entry is (class, virtual) -> (what the call site actually serves, where).
SHARED_OWNERSHIP_FALSE_CREDITS = {
    # scene_tree.rs draws a PROCEDURAL TEXTURE and takes its size from the
    # script. The rect is a Rect2 and the comment says "a 64x32 texture in a
    # larger rect still draws 64x32" — this is the Texture2D surface. NEITHER
    # registered owner is served, and Texture2D does not register these at all,
    # so the one place the engine calls `_get_width` is for a class ClassDB
    # never advertises it on.
    ("Texture3D", "_get_width"): ("a Texture2D-shaped script", "gdscene/src/scene_tree.rs:5226"),
    ("TextureLayered", "_get_width"): ("a Texture2D-shaped script", "gdscene/src/scene_tree.rs:5226"),
    ("Texture3D", "_get_height"): ("a Texture2D-shaped script", "gdscene/src/scene_tree.rs:5228"),
    ("TextureLayered", "_get_height"): ("a Texture2D-shaped script", "gdscene/src/scene_tree.rs:5228"),
    # `process_script_process(&mut self, node_id: NodeId, ..)` dispatches on a
    # NODE. MainLoop is the root ABOVE the tree, not a node in it, and
    # main_loop.rs contains no `"_process"` literal — only doc comments
    # describing when scripts run. MainLoop's own two virtuals are unreachable.
    ("MainLoop", "_process"): ("Node", "gdscene/src/scene_tree.rs:5547"),
    ("MainLoop", "_physics_process"): ("Node", "gdscene/src/scene_tree.rs:5552"),
    # visual_shader_custom.rs's `call_string("_get_name", ..)` is on the custom
    # visual-shader-node path.
    ("EditorSyntaxHighlighter", "_get_name"): (
        "VisualShaderNodeCustom",
        "gdscene/src/visual_shader_custom.rs:124",
    ),
    # editor_server.rs iterates `state.active_plugins`, so `_handles` reaches
    # EditorPlugin only; the two resource-preview/tooltip surfaces are not
    # consulted anywhere.
    ("EditorResourcePreviewGenerator", "_handles"): (
        "EditorPlugin",
        "gdeditor/src/editor_server.rs:5104",
    ),
    ("EditorResourceTooltipPlugin", "_handles"): (
        "EditorPlugin",
        "gdeditor/src/editor_server.rs:5104",
    ),
}

# The shared-AND-wired names, pinned. A 7th appearing means a new multi-owner
# virtual got a call site and nobody checked which owner it serves, so the guard
# fails and asks for that check rather than silently widening the blind spot.
VERIFIED_SHARED_WIRED = {
    "_get_height",
    "_get_name",
    "_get_width",
    "_handles",
    "_physics_process",
    "_process",
}

# Script virtuals registered in ClassDB that the engine never invokes, as of
# 2026-10-30. Shrinking this list is the point.
KNOWN_UNWIRED_VIRTUALS = [
    "_accessibility_get_contextual_info",
    "_activate_feed",
    "_begin_handle_action",
    "_can_be_hidden",
    "_can_drop_data",
    "_can_generate_small_preview",
    "_can_import_threaded",
    "_checkout_branch",
    "_clear_highlighting_cache",
    "_commit",
    "_commit_handle",
    "_commit_subgizmos",
    "_create",
    "_create_branch",
    "_create_gizmo",
    "_create_remote",
    "_deactivate_feed",
    "_discard_file",
    "_draw_editor_gizmo",
    "_drop_data",
    "_execute",
    "_exists",
    "_fetch",
    "_finalize",
    "_forward_3d_draw_over_viewport",
    "_forward_3d_force_draw_over_viewport",
    "_forward_canvas_draw_over_viewport",
    "_forward_canvas_force_draw_over_viewport",
    "_generate",
    "_generate_from_path",
    "_generate_small_preview_automatically",
    "_get_accessibility_configuration_warnings",
    "_get_accessibility_container_name",
    "_get_audio_mix_rate",
    "_get_audio_speaker_mode",
    "_get_branch_list",
    "_get_build_dependencies",
    "_get_channels",
    "_get_classes_used",
    "_get_current_branch_name",
    "_get_data",
    "_get_default_input_port",
    "_get_dependencies",
    "_get_depth",
    "_get_diff",
    "_get_drag_data",
    "_get_extensions",
    "_get_focused_accessibility_element",
    "_get_format",
    "_get_format_version",
    "_get_gizmo_name",
    "_get_handle_name",
    "_get_handle_value",
    "_get_import_options",
    "_get_import_order",
    "_get_importer_name",
    "_get_input_port_default_value",
    "_get_internal_import_options",
    "_get_internal_option_update_view_required",
    "_get_internal_option_visibility",
    "_get_joint_count",
    "_get_layer_data",
    "_get_layered_type",
    "_get_layers",
    "_get_length",
    "_get_line_diff",
    "_get_line_syntax_highlighting",
    "_get_minimum_size",
    "_get_mix_rate",
    "_get_modified_files_data",
    "_get_option_visibility",
    "_get_playback_position",
    "_get_plugin_icon",
    "_get_plural_message",
    "_get_preset_count",
    "_get_preset_name",
    "_get_previous_commits",
    "_get_priority",
    "_get_property_count",
    "_get_property_default_index",
    "_get_property_name",
    "_get_property_options",
    "_get_recognized_extensions",
    "_get_remotes",
    "_get_resource_script_class",
    "_get_resource_type",
    "_get_resource_uid",
    "_get_return_icon_type",
    "_get_rid",
    "_get_save_extension",
    "_get_subgizmo_transform",
    "_get_supported_languages",
    "_get_texture",
    "_get_tooltip",
    "_get_unsaved_status",
    "_get_vcs_name",
    "_get_visible_name",
    "_get_window_layout",
    "_handles_type",
    "_has_gizmo",
    "_has_mipmaps",
    "_has_point",
    "_import",
    "_import_scene",
    "_initialize",
    "_instantiate_playback",
    "_internal_process",
    "_is_handle_highlighted",
    "_is_paused",
    "_is_playing",
    "_is_selectable_when_hidden",
    "_load",
    "_make_custom_tooltip",
    "_make_tooltip_for_path",
    "_parse_file",
    "_play",
    "_post_process",
    "_pre_process",
    "_pull",
    "_push",
    "_recognize",
    "_recognize_path",
    "_redraw",
    "_remove_branch",
    "_remove_remote",
    "_rename_dependencies",
    "_render_callback",
    "_reset_state",
    "_run",
    "_run_scene",
    "_save",
    "_seek",
    "_set_aabb",
    "_set_audio_track",
    "_set_credentials",
    "_set_handle",
    "_set_normal",
    "_set_path_cache",
    "_set_paused",
    "_set_subgizmo_transform",
    "_set_uid",
    "_set_vertex",
    "_set_window_layout",
    "_setup_modification",
    "_shut_down",
    "_skeleton_changed",
    "_stage_file",
    "_stop",
    "_structured_text_parser",
    "_subgizmos_intersect_frustum",
    "_subgizmos_intersect_ray",
    "_tick",
    "_unstage_file",
    "_update",
    "_update_cache",
    "_validate_bone_names",
]


def registered_virtuals() -> dict[str, set[str]]:
    """{class_name: {virtual, ...}} for non-Extension classes."""
    src = open(CLASS_DB, encoding="utf-8", errors="replace").read()
    out: dict[str, set[str]] = {}
    for block in re.split(r'ClassRegistration::new\("', src)[1:]:
        name = block[: block.index('"')]
        if name.endswith("Extension"):
            continue
        virts = re.findall(r'MethodInfo::new\("(_[a-z_0-9]+)"', block[:20000])
        if virts:
            out.setdefault(name, set()).update(virts)
    return out


def call_site_corpus() -> list[tuple[str, str]]:
    files = [
        f
        for f in glob.glob(os.path.join(ENGINE, "crates/*/src/**/*.rs"), recursive=True)
        if not any(f.endswith(e) for e in NON_CALLING)
    ]
    return [(f, open(f, encoding="utf-8", errors="replace").read()) for f in files]


def unwired(per_class, corpus) -> list[str]:
    names = sorted({v for s in per_class.values() for v in s})
    return [n for n in names if not any(f'"{n}"' in t for _, t in corpus)]


def shared_wired(per_class, corpus) -> set[str]:
    """Virtuals with >1 non-Extension owner that DO have a call site.

    These are exactly the names where "wired" cannot be attributed to a class
    without reading the call site, which is why the set is pinned.
    """
    owners: dict[str, set[str]] = {}
    for cls, vs in per_class.items():
        for v in vs:
            owners.setdefault(v, set()).add(cls)
    miss = set(unwired(per_class, corpus))
    return {v for v, cs in owners.items() if len(cs) > 1 and v not in miss}


def pair_counts(per_class, corpus) -> tuple[int, int, int]:
    """(total pairs, unwired under per-name attribution, floor with false credits).

    The floor adds back only pairs CONFIRMED false by hand, so it stays a floor
    and never claims more than was checked.
    """
    pairs = {(c, v) for c, vs in per_class.items() for v in vs}
    miss = set(unwired(per_class, corpus))
    unw = {p for p in pairs if p[1] in miss}
    confirmed = {p for p in SHARED_OWNERSHIP_FALSE_CREDITS if p in pairs and p not in unw}
    return len(pairs), len(unw), len(unw) + len(confirmed)


def main() -> int:
    per_class = registered_virtuals()
    corpus = call_site_corpus()
    found = unwired(per_class, corpus)

    if "--report" in sys.argv:
        names = sorted({v for s in per_class.values() for v in s})
        miss = set(found)
        print(f"non-Extension classes registering virtuals: {len(per_class)}")
        print(f"distinct script-overridable virtuals: {len(names)}")
        print(f"never invoked by the engine: {len(miss)} ({100 * len(miss) // max(1, len(names))}%)")
        total, unw, floor = pair_counts(per_class, corpus)
        print(f"(class, virtual) pairs: {total}; unwired per-name {unw}, floor {floor}")
        print(f"  shared-and-wired names needing per-class reading: {len(shared_wired(per_class, corpus))}")
        for cls, vs in sorted(per_class.items()):
            m = sorted(v for v in vs if v in miss)
            if m:
                tag = "ALL" if len(m) == len(vs) else f"{len(m)}/{len(vs)}"
                print(f"  {cls:<36} {tag:>7} unwired")
        return 0

    baseline = set(KNOWN_UNWIRED_VIRTUALS)
    found_s = set(found)
    new = sorted(found_s - baseline)
    fixed = sorted(baseline - found_s)

    shared_now = shared_wired(per_class, corpus)
    unchecked = sorted(shared_now - VERIFIED_SHARED_WIRED)
    stale_shared = sorted(VERIFIED_SHARED_WIRED - shared_now)

    if not new and not fixed and not unchecked and not stale_shared:
        total, unw, floor = pair_counts(per_class, corpus)
        print(f"OK: {len(found_s)} unwired virtuals, exactly matching the pinned baseline")
        print(f"    per-class surface: {floor}-{total} of {total} (class, virtual) pairs unwired")
        return 0
    if unchecked:
        print(f"FAIL: {len(unchecked)} multi-owner virtual(s) became wired without a per-class")
        print("      check. Read the call site, decide which owner it serves, and either add")
        print("      the false credits to SHARED_OWNERSHIP_FALSE_CREDITS or note it serves all:")
        for n in unchecked:
            owners = sorted(c for c, vs in per_class.items() if n in vs)
            print(f"  - {n}   (owners: {', '.join(owners)})")
    if stale_shared:
        print(f"FAIL: {len(stale_shared)} pinned shared-wired virtual(s) no longer qualify —")
        print("      update VERIFIED_SHARED_WIRED (and drop any dead false credits):")
        for n in stale_shared:
            print(f"  - {n}")
    if new:
        print(f"FAIL: {len(new)} NEWLY unwired virtual(s) — registered in ClassDB but never")
        print("      invoked by the engine, so a script override can never run:")
        for n in new:
            owners = sorted(c for c, vs in per_class.items() if n in vs)
            print(f"  - {n}   (on {', '.join(owners)})")
    if fixed:
        print(f"FAIL: {len(fixed)} baselined virtual(s) are now WIRED — remove them from")
        print("      KNOWN_UNWIRED_VIRTUALS so the count stays honest:")
        for n in fixed:
            print(f"  - {n}")
    return 1


def self_test() -> int:
    """Prove the guard fails in both directions, not just vacuously passes."""
    per_class = registered_virtuals()
    corpus = call_site_corpus()
    real = set(unwired(per_class, corpus))
    bad = 0

    # (a) a newly-unwired virtual must be caught
    pretend = real - {sorted(real)[0]}
    if sorted(real - pretend):
        print("  caught: a newly unwired virtual")
    else:
        print("  NOT CAUGHT: a newly unwired virtual")
        bad += 1

    # (b) a baselined virtual that became wired must be caught
    pretend2 = real | {"_a_virtual_that_is_actually_called"}
    if sorted(pretend2 - real):
        print("  caught: a baselined virtual that is now wired")
    else:
        print("  NOT CAUGHT: a baselined virtual that is now wired")
        bad += 1

    # (c) the detector must find the two virtuals known to BE wired
    for known in ("_process_custom_fx", "_get_code"):
        if known in real:
            print(f"  NOT CAUGHT: {known} is wired but was reported unwired")
            bad += 1
        else:
            print(f"  correct: {known} detected as wired")

    # (d) excluding the registration table must matter
    all_files = [
        (f, open(f, encoding="utf-8", errors="replace").read())
        for f in glob.glob(os.path.join(ENGINE, "crates/*/src/**/*.rs"), recursive=True)
    ]
    if not unwired(per_class, all_files):
        print("  correct: counting class_db.rs would mark everything wired (exclusion matters)")
    else:
        print("  NOTE: including class_db.rs still leaves some unwired")
    # (e) every pair in the false-credit table must really be CREDITED as wired
    #     (i.e. NOT in the unwired set) — otherwise the entry is dead weight and
    #     the floor silently double-counts it.
    pairs = {(c, v) for c, vs in per_class.items() for v in vs}
    for pair, (serves, where) in sorted(SHARED_OWNERSHIP_FALSE_CREDITS.items()):
        cls, virt = pair
        if pair not in pairs:
            print(f"  STALE: {cls}.{virt} is no longer registered — drop the entry")
            bad += 1
        elif virt in real:
            print(f"  STALE: {cls}.{virt} is now reported unwired — drop the entry")
            bad += 1
        else:
            print(f"  confirmed false credit: {cls}.{virt} is served for {serves} ({where})")

    # (f) the shared-and-wired set must be exactly the 6 that were hand-checked,
    #     so a new multi-owner virtual cannot widen the blind spot unnoticed.
    sw = shared_wired(per_class, corpus)
    if sw == VERIFIED_SHARED_WIRED:
        print(f"  correct: {len(sw)} shared-and-wired virtuals, all hand-checked")
    else:
        print(f"  NOT CAUGHT: shared-wired set drifted to {sorted(sw)}")
        bad += 1

    # (g) the per-class floor must EXCEED the per-name count, which is the whole
    #     point of measuring it: the name metric understates the real surface.
    total, unw, floor = pair_counts(per_class, corpus)
    if floor > unw and floor == unw + len(SHARED_OWNERSHIP_FALSE_CREDITS):
        print(f"  correct: per-class floor {floor} > per-name {unw} of {total} pairs")
    else:
        print(f"  NOT CAUGHT: floor {floor} vs per-name {unw} (expected +9)")
        bad += 1

    print("self-test PASSED" if not bad else f"self-test FAILED ({bad})")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv else main())
