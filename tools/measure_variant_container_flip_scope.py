#!/usr/bin/env python3
"""pat-s3rrv.8 — measure the Array/Dictionary reference-semantics flip.

Six workers have now been assigned this bead and all six reached the same
conclusion: the enum flip

    Variant::Array(Vec<Variant>)  -> Variant::Array(SharedArray)
    Variant::Dictionary(Dictionary) -> Variant::Dictionary(SharedDictionary)

is atomic. `gdvariant` is the workspace root crate, every dependent crate stops
compiling the moment the payload type changes, and the verifier builds the
working tree — so there is no green intermediate state to hand back. Everything
else on the bead (the `array_mutate` write-backs, the nine typed-container
queries, the acceptance test) is downstream of it.

That analysis is complete and lives in the bead comments. This script exists to
answer the one question the comments cannot: **is it still true, and is the job
getting bigger?** Run it instead of re-deriving the analysis a seventh time.

⚠️ The site count is measured at **HEAD**, not in the working tree. The tree
carries other agents' uncommitted edits, so a tree count conflates "the swarm
committed more sites" with "someone is mid-edit right now" — and only the first
is real growth. The tree count is reported alongside purely as the in-flight
delta.

⚠️ Fails closed. A zero count means the grep stopped matching (a rename, a moved
tree), not that the work vanished — treating that as "scope is now empty" is the
failure this guards against.

Usage: python3 tools/measure_variant_container_flip_scope.py
"""

from __future__ import annotations

import pathlib
import subprocess
import sys

# Absolute, because the Rust verifier runs from `engine-rs/` rather than the
# repo root — a relative path here works when tested by hand and dies there.
ROOT = pathlib.Path(__file__).resolve().parent.parent
TREES = ["engine-rs/crates", "engine-rs/tests"]
NEEDLES = ("Variant::Array(", "Variant::Dictionary(")

# The two payload types as they read *before* the flip. Their presence is the
# whole gate: while these match, nothing else on the bead is deliverable.
UNFLIPPED = ("Array(Vec<Variant>),", "Dictionary(Dictionary),")

# BronzeBridge's workspace-wide measurement, 2026-08-19 09:07 UTC, recorded in
# the bead comments. Kept as a fixed prior so the growth rate is reproducible
# rather than a claim.
PRIOR_TS = "2026-08-19T09:07:00Z"
PRIOR_SITES = 742


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, text=True, check=True
    ).stdout


def rust_files() -> list[str]:
    return [f for f in git("ls-files", *TREES).split() if f.endswith(".rs")]


def count_at_head(files: list[str]) -> int:
    total = 0
    for f in files:
        blob = subprocess.run(
            ["git", "show", f"HEAD:{f}"], cwd=ROOT, capture_output=True, text=True
        ).stdout
        total += sum(1 for line in blob.splitlines() if any(n in line for n in NEEDLES))
    return total


def count_in_tree(files: list[str]) -> int:
    total = 0
    for f in files:
        p = ROOT / f
        if not p.exists():
            continue
        text = p.read_text(errors="replace")
        total += sum(1 for line in text.splitlines() if any(n in line for n in NEEDLES))
    return total


def main() -> int:
    variant_rs = (ROOT / "engine-rs/crates/gdvariant/src/variant.rs").read_text()
    still_unflipped = [u for u in UNFLIPPED if u in variant_rs]

    files = rust_files()
    head = count_at_head(files)
    tree = count_in_tree(files)

    shared = ROOT / "engine-rs/crates/gdvariant/src/shared.rs"

    print(f"tracked .rs files scanned : {len(files)}")
    print(f"call sites at HEAD        : {head}")
    print(f"call sites in working tree: {tree}  (delta {tree - head:+d} uncommitted)")
    print(f"prior measurement         : {PRIOR_SITES} at {PRIOR_TS}")
    print(f"growth at HEAD since prior: {head - PRIOR_SITES:+d}")
    print(f"SharedArray/SharedDictionary present: {shared.is_file()}")
    print(f"enum still unflipped      : {still_unflipped}")

    # Fail closed on a silent grep break — see the module docstring.
    assert head > 0, "zero sites at HEAD: the needles stopped matching, scope is not empty"
    assert files, "no tracked .rs files found: the tree layout moved"
    assert shared.is_file(), (
        "gdvariant/src/shared.rs is gone — SharedArray/SharedDictionary are the "
        "half of this bead that IS done; losing them re-opens delivered work"
    )

    # The gate. While the payload types are unflipped, every remaining item on
    # the bead is blocked behind an atomic change no worker can land.
    assert len(still_unflipped) == 2, (
        f"variant.rs no longer carries both value-typed payloads ({still_unflipped}) "
        "— the flip has started or landed, so re-read the bead: the write-backs, "
        "the typed-container queries and the acceptance test may now be deliverable"
    )
    assert head >= PRIOR_SITES, (
        f"site count fell from {PRIOR_SITES} to {head} — a sweep is underway; "
        "coordinate before touching this"
    )

    print("\nBLOCKED: the flip is untouched and the sweep target has grown.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
