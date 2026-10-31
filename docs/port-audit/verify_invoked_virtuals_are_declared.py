#!/usr/bin/env python3
"""Guard: a virtual the ENGINE INVOKES must be DECLARED in ClassDB.

This is the INVERSE of `verify_classdb_virtuals_are_dispatched.py`, and it finds
a different bug. That guard asks "we declared it — does anything call it?" and
its failure is the Nil placeholder: `has_method` true, call returns `Nil`
silently. **This one asks "we call it — did we declare it?"** and its failure is
the opposite: the engine runs the override while ClassDB denies the method
exists, so a script written defensively as

    if has_method("_get_width"): ...

skips an override that would have worked. Nothing errors, because the engine's
call sites use `.ok()` / `has_method` guards with working fallbacks — which is
precisely what makes this class of gap invisible to the test suite.

FIXED SO FAR VIA THIS CHECK (2026-10-30), baseline 13 -> 9:
  * `Texture2D._get_width`/`_get_height`/`_draw_rect` — a procedural texture's
    whole size-and-draw surface.
  * `Object._get`/`_set`/`_get_property_list`/`_to_string` — the four widest
    rows in the baseline, since Object is the ROOT and every class inherits
    them. `gdscene/src/scripting.rs` has an "Object virtual chain" section
    dispatching all four, so `class_has_method(<any class>, "_get")` answered
    false for a virtual the engine runs on every object.

HOW THE FIRST ONE WAS FOUND: `Texture2D` declared THREE invoked virtuals as
zero. `_get_width`/`_get_height`/`_draw_rect` are called on a procedural texture
at `gdscene/src/scene_tree.rs:5226/5228/5235`, and the class registered three
non-virtual methods and no virtuals at all, against Godot's seven. Fixed under
pat-nzgdu; this guard exists so the next one is caught by a command instead of
by reading the file.

🔑 ORACLE-GATED, which is what makes every row actionable. A name is reported
only if ALL THREE hold:
  1. the engine passes it as a literal to a script-dispatch helper, AND
  2. `upstream/godot/doc/classes/*.xml` declares it as a `virtual` method, AND
  3. NO class in our `class_db.rs` declares it — not one, including `*Extension`.

Condition (2) is load-bearing and was added after measuring without it. The raw
scan reports 18 names; five are NOT virtuals upstream and would be false
positives that a maintainer would have to re-triage by hand every run.

⚠️ MEASURED, so the gate is not described as stronger than it is: of the 64
invoked names, 59 are documented methods and **all 59 of those carry the
`virtual` attribute**. So today the discriminator doing the work is "is this a
documented `<method>` at all", and the `virtual` attribute check excludes ZERO
additional names. It is kept because a non-virtual `_`-prefixed method is a
thing Godot can document and a script cannot override, but it should not be
credited with the filtering the method check is actually doing.

The five rejected names:
  * `_focus_entered`, `_focus_exited`, `_mouse_entered`, `_mouse_exited` are
    Godot **signal** names, not virtuals. Patina invokes them as script
    callbacks, which is a local convention; declaring them as ClassDB methods
    would be wrong.
  * `_nonexistent` is a TEST FIXTURE at `gdscene/src/scripting.rs:8709` — an
    in-file `mod tests` deliberately calling a missing method. The oracle gate
    removes it without my having to strip `#[cfg(test)]` regions, which a
    textual scan cannot do reliably.

Condition (3) is deliberately "declared NOWHERE", not "declared on the right
class". Per-class attribution needs the call site read by hand (see
SHARED_OWNERSHIP_FALSE_CREDITS in the companion guard for why), so this guard
reports only the unambiguous case and stays sound without judgement calls. A
virtual declared on the WRONG class still passes here — that is a known,
documented limit, not an oversight.

Usage:
    python3 docs/port-audit/verify_invoked_virtuals_are_declared.py
    python3 docs/port-audit/verify_invoked_virtuals_are_declared.py --report
    python3 docs/port-audit/verify_invoked_virtuals_are_declared.py --self-test
"""
from __future__ import annotations

import glob
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ENGINE = os.path.join(REPO, "engine-rs")
CLASS_DB = os.path.join(ENGINE, "crates/gdobject/src/class_db.rs")
DOC_CLASSES = os.path.join(REPO, "upstream/godot/doc/classes")

# Helpers that dispatch a named method into a script. A literal passed to one of
# these is the engine INVOKING that virtual.
DISPATCH_HELPERS = (
    "call_method",
    "call_script_method",
    "call_script_with_access",
    "has_method",
    "draw_with_external_script",
    "call_string",
    "call_script",
    "script_call",
    "call_virtual",
    "invoke_script",
)

# Files that NAME methods without calling them.
NON_CALLING = (
    "class_db.rs",
    "inspector_schema_generated.rs",
    "create_node_catalog.rs",
    "create_dialog.rs",
)

# Virtuals the engine invokes that NO class declares in ClassDB, as of
# 2026-10-30, with the upstream class that owns each and its documented arity.
#
# ⚠️ Every row is a live defect, not a wishlist: the engine already calls these,
# so each one is a script override that `has_method` reports as unavailable.
# Shrinking this list is the point. `Object._get`/`_set`/`_get_property_list`/
# `_to_string` are the widest-reaching — they apply to every object.
KNOWN_UNDECLARED = {
    "_draw": ("CanvasItem/StyleBox/Texture2D", 4),
    "_input_event": ("CollisionObject2D/CollisionObject3D", 5),
    "_integrate_forces": ("RigidBody2D/RigidBody3D/PhysicalBone3D", 0),
    "_mouse_enter": ("CollisionObject2D/CollisionObject3D", 0),
    "_mouse_exit": ("CollisionObject2D/CollisionObject3D", 0),
    "_mouse_shape_enter": ("CollisionObject2D", 1),
    "_mouse_shape_exit": ("CollisionObject2D", 1),
    "_pressed": ("BaseButton", 0),
    "_toggled": ("BaseButton", 1),
}

# Names the raw scan finds that the ORACLE GATE correctly rejects. Pinned so the
# gate's value is asserted rather than described: if one of these starts being
# reported, condition (2) has broken.
ORACLE_REJECTS = {
    "_focus_entered": "a Godot signal, not a virtual",
    "_focus_exited": "a Godot signal, not a virtual",
    "_mouse_entered": "a Godot signal, not a virtual",
    "_mouse_exited": "a Godot signal, not a virtual",
    "_nonexistent": "a test fixture in an in-file `mod tests`",
}


def _call_args(src: str, open_paren: int) -> str:
    """The text of one call's argument list, honouring NESTED parens.

    ⚠️ A regex cannot do this, and getting it wrong silently SHRINKS the
    finding set rather than erroring. An earlier version of this function
    matched `[^()"]*` after the helper name, which stopped at the first nested
    paren — so `draw_with_external_script(*node_id, Box::new(script),
    "_draw_rect", ..)` lost its method literal entirely and `_draw_rect` was
    invisible to the scan. The guard's own self-test caught it by asserting that
    the three virtuals this audit already fixed are still SEEN as invoked.
    """
    depth, i, n = 0, open_paren, len(src)
    while i < n:
        c = src[i]
        if c == '"':  # skip a string literal, escapes included
            i += 1
            while i < n and src[i] != '"':
                i += 2 if src[i] == "\\" else 1
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return src[open_paren : i + 1]
        i += 1
    return src[open_paren:]


def invoked_virtuals() -> dict[str, list[str]]:
    """{virtual: [file:line, ...]} for literals passed to a dispatch helper."""
    out: dict[str, list[str]] = {}
    names = "|".join(re.escape(h) for h in DISPATCH_HELPERS)
    for path in glob.glob(os.path.join(ENGINE, "crates/*/src/**/*.rs"), recursive=True):
        if any(path.endswith(e) for e in NON_CALLING):
            continue
        src = open(path, encoding="utf-8", errors="replace").read()
        rel = os.path.relpath(path, os.path.join(ENGINE, "crates"))
        for m in re.finditer(r"\b(?:" + names + r")\s*\(", src):
            args = _call_args(src, m.end() - 1)
            line = src[: m.start()].count("\n") + 1
            for v in re.findall(r'"(_[a-z_0-9]+)"', args):
                out.setdefault(v, []).append(f"{rel}:{line}")
    return out


def upstream_virtuals() -> dict[str, tuple[set[str], int]]:
    """{virtual: ({owning class, ...}, arity)} from the Godot doc XML."""
    out: dict[str, tuple[set[str], int]] = {}
    for path in glob.glob(os.path.join(DOC_CLASSES, "*.xml")):
        cls = os.path.basename(path)[:-4]
        x = open(path, encoding="utf-8", errors="replace").read()
        for m in re.finditer(r'<method name="(_[a-z_0-9]+)"([^>]*)>(.*?)</method>', x, re.S):
            name, attrs, body = m.group(1), m.group(2), m.group(3)
            if "virtual" not in attrs:
                continue
            argc = len(
                re.findall(r'<param index="\d+" name="[a-z_0-9]+" type="[A-Za-z0-9_]+"', body)
            )
            owners, prev = out.get(name, (set(), argc))
            owners.add(cls)
            out[name] = (owners, prev)
    return out


def declared_anywhere() -> set[str]:
    src = open(CLASS_DB, encoding="utf-8", errors="replace").read()
    return set(re.findall(r'MethodInfo::new\("(_[a-z_0-9]+)"', src))


def gaps(invoked, upstream, declared) -> dict[str, list[str]]:
    """Invoked AND an upstream virtual AND declared on no class at all."""
    return {
        v: sites
        for v, sites in invoked.items()
        if v in upstream and v not in declared
    }


def main() -> int:
    invoked = invoked_virtuals()
    upstream = upstream_virtuals()
    declared = declared_anywhere()
    found = gaps(invoked, upstream, declared)

    if "--report" in sys.argv:
        print(f"virtuals invoked via a dispatch helper: {len(invoked)}")
        print(f"of those, declared as virtual upstream: {sum(1 for v in invoked if v in upstream)}")
        print(f"declared somewhere in our ClassDB: {len(declared)}")
        print(f"\nINVOKED BUT DECLARED NOWHERE: {len(found)}")
        for v, sites in sorted(found.items()):
            owners, argc = upstream[v]
            print(f"  {v:<22} arity {argc}  upstream: {'/'.join(sorted(owners)[:3])}")
            print(f"      called at {sorted(set(sites))[:2]}")
        rejected = sorted(v for v in invoked if v not in upstream)
        print(f"\nrejected by the oracle gate (not virtuals upstream): {len(rejected)}")
        for v in rejected:
            print(f"  {v:<22} {ORACLE_REJECTS.get(v, '(unclassified)')}")
        return 0

    new = sorted(set(found) - set(KNOWN_UNDECLARED))
    fixed = sorted(set(KNOWN_UNDECLARED) - set(found))

    if not new and not fixed:
        print(f"OK: {len(found)} invoked-but-undeclared virtuals, matching the pinned baseline")
        return 0
    if new:
        print(f"FAIL: {len(new)} virtual(s) the engine INVOKES are declared on NO class.")
        print("      `has_method` answers false, so a script that guards with it skips an")
        print("      override the engine would have run — silently, because the call sites")
        print("      have working fallbacks:")
        for v in new:
            owners, argc = upstream[v]
            print(f"  - {v} (arity {argc}, upstream {'/'.join(sorted(owners)[:3])})")
            print(f"      called at {sorted(set(found[v]))[:2]}")
    if fixed:
        print(f"FAIL: {len(fixed)} baselined virtual(s) are now DECLARED — remove them from")
        print("      KNOWN_UNDECLARED so the count stays honest:")
        for v in fixed:
            print(f"  - {v}")
    return 1


def self_test() -> int:
    """Prove the gate and both failure directions, not just a vacuous pass."""
    invoked = invoked_virtuals()
    upstream = upstream_virtuals()
    declared = declared_anywhere()
    real = gaps(invoked, upstream, declared)
    bad = 0

    # (a) the find that motivated the guard: Texture2D's three virtuals are
    #     declared NOW, so they must NOT be reported — and they must still be
    #     invoked, or the guard has stopped looking at the right call sites.
    for fixed_name in ("_get_width", "_get_height", "_draw_rect"):
        if fixed_name not in invoked:
            print(f"  NOT CAUGHT: {fixed_name} is no longer seen as invoked — scan broke")
            bad += 1
        elif fixed_name in real:
            print(f"  NOT CAUGHT: {fixed_name} is declared but still reported")
            bad += 1
        else:
            print(f"  correct: {fixed_name} invoked and declared, so not reported")

    # (b) the oracle gate must reject every known non-virtual
    for name, why in sorted(ORACLE_REJECTS.items()):
        if name not in invoked:
            print(f"  NOTE: {name} is no longer invoked at all ({why})")
        elif name in real:
            print(f"  NOT CAUGHT: {name} reported though it is {why}")
            bad += 1
        else:
            print(f"  correct: oracle gate rejects {name} — {why}")

    # (c) a NEW undeclared invoked virtual must be caught
    pretend = {v: s for v, s in real.items() if v != sorted(real)[0]}
    if sorted(set(real) - set(pretend)):
        print("  caught: a newly undeclared invoked virtual")
    else:
        print("  NOT CAUGHT: a newly undeclared invoked virtual")
        bad += 1

    # (d) a baselined virtual that becomes declared must ALSO be caught, so the
    #     list shrinks honestly rather than being left stale.
    # ⚠️ Uses a name still IN the baseline. An earlier version used
    # `_to_string`, which this audit then fixed — at which point the check was
    # simulating something already true and proved nothing.
    pretend_declared = {v: s for v, s in real.items() if v != "_pressed"}
    if sorted(set(KNOWN_UNDECLARED) - set(pretend_declared)) == ["_pressed"]:
        print("  caught: a baselined virtual that is now declared")
    else:
        print("  NOT CAUGHT: a baselined virtual that is now declared")
        bad += 1

    # (e) the baseline's recorded arities must match the oracle, since a fix
    #     transcribes them — a wrong arity here would be copied into ClassDB,
    #     where it still satisfies `has_method` and fails inside the script.
    for v, (_, argc) in sorted(KNOWN_UNDECLARED.items()):
        if v not in upstream:
            print(f"  STALE: {v} is no longer an upstream virtual — drop the row")
            bad += 1
        elif upstream[v][1] != argc:
            print(f"  WRONG ARITY: {v} baselined {argc}, doc XML says {upstream[v][1]}")
            bad += 1
    print(f"  checked {len(KNOWN_UNDECLARED)} baselined arities against the doc XML")

    print("self-test PASSED" if not bad else f"self-test FAILED ({bad})")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv else main())
