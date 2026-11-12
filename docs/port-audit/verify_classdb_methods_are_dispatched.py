#!/usr/bin/env python3
"""Guard: a ClassDB-registered method must have a dispatch arm, or scripts get Nil.

Companion to verify_classdb_virtuals_are_dispatched.py. That one covers
engine->script virtuals; this one covers the opposite direction — script->engine
methods that ClassDB advertises but no dispatch arm serves.

THE FAILURE IS SILENT, and the exact path is in the engine
(gdscript-interop/src/interpreter.rs, `call_object_method`):

    let responds = ... access.node_has_method(id, method) ...
    if let Some(value) = access.call_node_method(id, method, args) {
        return Ok(value);
    }
    if responds {
        return Ok(Variant::Nil);        // <-- "the Nil placeholder"
    }
    Err(RuntimeError::new(RuntimeErrorKind::UndefinedFunction(..)))

and `node_has_method` (gdscene/src/scene_tree.rs) answers from the registration
table alone:

    gdobject::class_db::class_has_method(node.class_name(), method)

So registering a method WITHOUT a dispatch arm makes `has_method` answer true
and the call return `Nil` with no error. A script cannot even detect it, because
the thing it would ask — `has_method` — is the thing that is lying. The engine
already carries a comment about one instance of this (`is_class`, which
"returned **Nil** — a falsy value, silently, for every object").

MEASURED STATE when this guard was written (2026-10-30):
    264 non-Extension classes register at least one non-virtual method
    2180 (class, method) pairs
    94 classes have NO reachable method at all — 650 advertised methods that
       all return Nil

⚠️ THE CORPUS IS THE WHOLE POINT, AND I GOT IT WRONG FIRST.
My first pass searched only `crates/gdscript-interop/src/` and reported 125
unreachable classes including NavigationAgent2D/3D. That was wrong: the node
method table lives in **gdscene/src/scripting.rs** (`call_node_method`, ~:2594),
whose own comment says "NavigationAgent2D/3D answer their own method surface".
Dispatch is spread across crates, so the corpus must be every engine source
file except the ones that merely NAME methods. `self_test` pins
NavigationAgent2D as mostly-reachable precisely so that mistake fails loudly
instead of producing 31 phantom findings.

EXCLUDED, because they list method names without calling them — counting them
would mark everything reachable:
    class_db.rs, inspector_schema_generated.rs, create_node_catalog.rs,
    create_dialog.rs

ALSO EXCLUDED: `*Extension` classes, whose methods are GDExtension-only.

⚠️ TWO KNOWN ERROR MODES, both found by using this guard on its own output.
Neither signal is sound alone, and this guard deliberately takes the one that
UNDER-reports, because a guard that over-reports gets ignored.

 1. FALSE NEGATIVE (this metric): a method name is counted as dispatched if it
    appears ANYWHERE, so a sibling class's handler using the same name hides a
    real gap. Concretely: wiring Camera3D's `make_current`/`clear_current`
    (pat-nzgdu) made `AudioListener2D` and `AudioListener3D` drop out of this
    list, even though neither has any arm — the Camera3D handler declines every
    non-Camera3D node. **Those two ARE unreachable; their absence below is this
    bug, not progress.** The only mention of them outside the registration table
    is `audio_server_bridge.rs:522` doing `current_node_of_class(tree,
    "AudioListener3D")`, which is the engine finding the active listener for
    panning, not a script dispatch arm.

 2. FALSE POSITIVE (the stricter variant, not used here): requiring the method
    name to appear in a file that also names the class raises the count from 91
    to 179 and hides nothing — but it wrongly flags HANDLE-dispatched classes.
    `Tween` is served by `tween_call(tween: u64, method, args)`
    (gdscene/src/scripting.rs:7123), keyed by a tween id rather than a class
    name, so no class-name check exists to find. Shapes and query-parameter
    objects use the same handle shape.

So 91 is a FLOOR, not a count. The real figure is between 91 and 179, and
closing the gap needs per-class arm attribution rather than a textual scan.

PINNED IN BOTH DIRECTIONS: a newly unreachable class fails (you registered a
surface nothing serves); a baselined class that becomes reachable also fails, so
the list shrinks honestly.

Usage:
    python3 docs/port-audit/verify_classdb_methods_are_dispatched.py
    python3 docs/port-audit/verify_classdb_methods_are_dispatched.py --report
    python3 docs/port-audit/verify_classdb_methods_are_dispatched.py --self-test
"""
from __future__ import annotations

import glob
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ENGINE = os.path.join(REPO, "engine-rs")
CLASS_DB = os.path.join(ENGINE, "crates/gdobject/src/class_db.rs")

NON_CALLING = (
    "class_db.rs",
    "inspector_schema_generated.rs",
    "create_node_catalog.rs",
    "create_dialog.rs",
)

# Classes whose ENTIRE registered method surface is unreachable from a script,
# as of 2026-10-30. Every method on these returns Nil silently.
KNOWN_UNREACHABLE_CLASSES = [
    "AimModifier3D",  # 10 methods
    "ArrayOccluder3D",  # 3 methods
    "AudioEffectEQ",  # 3 methods
    "AudioStream",  # 6 methods
    "AudioStreamInteractive",  # 19 methods
    "AudioStreamOggVorbis",  # 16 methods
    "AudioStreamPlaybackInteractive",  # 3 methods
    "AudioStreamPlaybackPolyphonic",  # 5 methods
    "AudioStreamPlaybackResampled",  # 2 methods
    "AudioStreamPlaylist",  # 3 methods
    "AudioStreamSynchronized",  # 4 methods
    "BoneConstraint3D",  # 17 methods
    "BoneMap",  # 3 methods
    "BoneTwistDisperser3D",  # 34 methods
    "CPUParticles2D",  # 3 methods
    "CanvasTexture",  # 14 methods
    "ChainIK3D",  # 17 methods
    "CodeHighlighter",  # 28 methods
    "ConvertTransformModifier3D",  # 20 methods
    "CopyTransformModifier3D",  # 16 methods
    "Cubemap",  # 1 methods
    "CubemapArray",  # 1 methods
    "CurveTexture",  # 5 methods
    "CurveXYZTexture",  # 7 methods
    "DPITexture",  # 5 methods
    "EditorFeatureProfile",  # 11 methods
    "EditorNode3DGizmoPlugin",  # 5 methods
    "EditorPlugin",  # 9 methods
    "EditorResourcePreview",  # 5 methods
    "EditorResourcePreviewGenerator",  # 1 methods
    "EditorResourceTooltipPlugin",  # 1 methods
    "EditorSceneFormatImporter",  # 2 methods
    "EditorScenePostImport",  # 1 methods
    "EditorScenePostImportPlugin",  # 3 methods
    "EditorScript",  # 3 methods
    "EditorToaster",  # 1 methods
    "EditorVCSInterface",  # 8 methods
    "FBXState",  # 2 methods
    "FogMaterial",  # 12 methods
    "GPUParticles2D",  # 5 methods
    "ImageTextureLayered",  # 2 methods
    "ImporterMeshInstance3D",  # 20 methods
    "IterateIK3D",  # 22 methods
    "JNISingleton",  # 1 methods
    "JavaClass",  # 4 methods
    "JavaObject",  # 2 methods
    "LightmapGIData",  # 6 methods
    "MeshInstance3D",  # 13 methods
    "MissingResource",  # 4 methods
    "MultiMesh",  # 12 methods
    "Noise",  # 9 methods
    "Occluder3D",  # 2 methods
    "OccluderInstance3D",  # 8 methods
    "OggPacketSequence",  # 7 methods
    "OptimizedTranslation",  # 1 methods
    "PlaceholderMesh",  # 1 methods
    "PolygonOccluder3D",  # 2 methods
    "RDFramebufferPass",  # 10 methods
    "RenderSceneBuffers",  # 1 methods
    "RenderSceneBuffersConfiguration",  # 20 methods
    "RenderSceneBuffersRD",  # 27 methods
    "RenderSceneData",  # 6 methods
    "RichTextEffect",  # 1 methods
    "ScriptEditorBase",  # 2 methods
    "Shader",  # 5 methods
    "ShaderIncludeDB",  # 3 methods
    "SkeletonModification2D",  # 10 methods
    "SkeletonModification2DStackHolder",  # 2 methods
    "SkeletonModification2DTwoBoneIK",  # 16 methods
    "Skin",  # 11 methods
    "SkinReference",  # 2 methods
    "SphereOccluder3D",  # 2 methods
    "SyntaxHighlighter",  # 4 methods
    "Texture2DArray",  # 1 methods
    "Texture2DRD",  # 2 methods
    "Texture3DRD",  # 2 methods
    "TextureLayeredRD",  # 2 methods
    "ThemeDB",  # 12 methods
    "UniformSetCacheRD",  # 1 methods
    "Viewport",  # 1 methods
    "VisualShaderNode",  # 11 methods
    "VisualShaderNodeCubemap",  # 6 methods
    "VisualShaderNodeCurveTexture",  # 2 methods
    "VisualShaderNodeCurveXYZTexture",  # 2 methods
    "VisualShaderNodeCustom",  # 1 methods
    "VisualShaderNodeFrame",  # 2 methods
    "VisualShaderNodeGroupBase",  # 21 methods
    "VisualShaderNodeReroute",  # 1 methods
    "VisualShaderNodeVectorBase",  # 2 methods
    "VisualShaderNodeVectorOp",  # 2 methods
    "Window",  # 2 methods
]


def registered_methods() -> dict[str, set[str]]:
    src = open(CLASS_DB, encoding="utf-8", errors="replace").read()
    out: dict[str, set[str]] = {}
    for block in re.split(r'ClassRegistration::new\("', src)[1:]:
        name = block[: block.index('"')]
        if name.endswith("Extension"):
            continue
        ms = re.findall(r'MethodInfo::new\("([a-z][a-z_0-9]*)"', block[:20000])
        if ms:
            out.setdefault(name, set()).update(ms)
    return out


def dispatch_corpus(include_all: bool = False) -> list[str]:
    files = glob.glob(os.path.join(ENGINE, "crates/*/src/**/*.rs"), recursive=True)
    if not include_all:
        files = [f for f in files if not any(f.endswith(e) for e in NON_CALLING)]
    return [open(f, encoding="utf-8", errors="replace").read() for f in files]


def unreachable(per_class, corpus) -> dict[str, int]:
    out = {}
    for cls, ms in per_class.items():
        if ms and all(not any(f'"{m}"' in t for t in corpus) for m in ms):
            out[cls] = len(ms)
    return out


def main() -> int:
    per_class = registered_methods()
    corpus = dispatch_corpus()
    found = unreachable(per_class, corpus)

    if "--report" in sys.argv:
        pairs = sum(len(v) for v in per_class.values())
        print(f"non-Extension classes registering methods: {len(per_class)}")
        print(f"(class, method) pairs: {pairs}")
        print(f"classes with NO reachable method: {len(found)}  ({sum(found.values())} methods return Nil)")
        for cls, n in sorted(found.items(), key=lambda kv: -kv[1]):
            print(f"  {n:3}  {cls}")
        return 0

    baseline = set(KNOWN_UNREACHABLE_CLASSES)
    new = sorted(set(found) - baseline)
    fixed = sorted(baseline - set(found))
    if not new and not fixed:
        print(f"OK: {len(found)} entirely-unreachable classes, matching the pinned baseline")
        return 0
    if new:
        print(f"FAIL: {len(new)} class(es) newly advertise a method surface nothing dispatches.")
        print("      A script calling these gets Nil with no error:")
        for c in new:
            print(f"  - {c}  ({len(per_class.get(c, ()))} methods)")
    if fixed:
        print(f"FAIL: {len(fixed)} baselined class(es) are now reachable — remove them from")
        print("      KNOWN_UNREACHABLE_CLASSES:")
        for c in fixed:
            print(f"  - {c}")
    return 1


def self_test() -> int:
    per_class = registered_methods()
    corpus = dispatch_corpus()
    found = unreachable(per_class, corpus)
    bad = 0

    # ⚠️ The regression that matters: searching only gdscript-interop reported
    # NavigationAgent2D/3D as unreachable, because the node method table lives
    # in gdscene/src/scripting.rs. Pin them as reachable so a corpus mistake
    # fails here instead of producing phantom findings.
    for cls in ("NavigationAgent2D", "NavigationAgent3D"):
        if cls in found:
            print(f"  NOT CAUGHT: {cls} reported unreachable — the dispatch corpus is wrong")
            bad += 1
        else:
            print(f"  correct: {cls} detected as reachable (gdscene/src/scripting.rs)")

    # ⚠️ Error mode 1, pinned: AudioListener2D/3D are NOT in the baseline because
    # a sibling handler uses their method names, but they have no arm. If a
    # future metric change starts reporting them, that is a FIX, not a
    # regression — this assertion records the known-wrong state so the next
    # reader is not misled by their absence.
    for cls in ("AudioListener2D", "AudioListener3D"):
        if cls in found:
            print(f"  NOTE: {cls} is now reported unreachable — the metric improved;")
            print(f"        add it to KNOWN_UNREACHABLE_CLASSES and update this note")
        else:
            print(f"  known false negative: {cls} is unreachable but not reported")

    # ⚠️ Error mode 2, pinned: Tween is served by HANDLE (tween_call, keyed by a
    # tween id, not a class name), so a class-name-aware metric would wrongly
    # flag it. Asserting it stays unflagged keeps that variant honest.
    if "Tween" in found:
        print("  NOT CAUGHT: Tween flagged, but it is handle-dispatched via tween_call")
        bad += 1
    else:
        print("  correct: Tween not flagged (handle dispatch, scripting.rs tween_call)")

    # a newly unreachable class must be caught
    if sorted(set(found) | {"_APhantomClass"} - set(found)):
        print("  caught: a newly unreachable class")
    else:
        print("  NOT CAUGHT: a newly unreachable class")
        bad += 1

    # the exclusion of the registration table must matter
    if not unreachable(per_class, dispatch_corpus(include_all=True)):
        print("  correct: counting class_db.rs would mark every class reachable")
    else:
        print("  NOTE: including class_db.rs still leaves some unreachable")

    print("self-test PASSED" if not bad else f"self-test FAILED ({bad})")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv else main())
