#!/usr/bin/env python3
"""Falsifiable check for COMPAT_MATRIX.md's Phase 9 Hardening Artifact Classification.

A docs-only change still owes a command that can FAIL. This ports the six
assertions of `misc_suite_e::phase9_hardening_classification_test` that were red
at HEAD (all on one cause: the section did not exist) so the claim can be
falsified without a Rust build, which workers are not permitted to run.

It is deliberately stricter than the Rust test in one respect: every `_test.rs`
path cited as evidence must EXIST ON DISK. The Rust test only greps for the
substring `_test.rs`, so it would happily accept a citation of a file that was
never written or has since moved -- and every path in the audit's own source list
is stale for exactly that reason.

Usage:
    python3 docs/test-debt/verify_phase9_classification.py            # exit 0 / 1
    python3 docs/test-debt/verify_phase9_classification.py --self-test
        Injects three false rows into an in-memory copy and asserts each is
        caught, proving the checker is not vacuously green.

Exit 0 = every assertion holds. Exit 1 = at least one failed (reasons printed).
"""
from __future__ import annotations

import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MATRIX = os.path.join(REPO, "COMPAT_MATRIX.md")
AUDIT = os.path.join(REPO, "prd", "PHASE9_HARDENING_AUDIT.md")

HEADING = "## Phase 9 Hardening Artifact Classification"
REQUIRED = [
    "Benchmark dashboards",
    "Fuzz/property tests",
    "Crash triage process",
    "Release train",
    "Contributor onboarding",
    "Migration guide",
]
APPROVED = ["Measured", "Implemented, not yet measured", "Deferred", "Missing"]


def hardening_section(matrix: str) -> str:
    """Port of the Rust helper at phase9_hardening_classification_test.rs:29."""
    start = matrix.find(HEADING)
    if start < 0:
        return ""
    section = matrix[start:]
    ends = [i + 1 for i in (section[1:].find("\n## "), section[1:].find("\n---\n")) if i >= 0]
    return section[: min(ends)] if ends else section


def table_rows(section: str) -> list[str]:
    """Rows the Rust test counts: pipe-led, minus header and separator."""
    return [
        line
        for line in section.splitlines()
        if line.startswith("|")
        and "Deliverable" not in line
        and "Classification" not in line
        and "---" not in line
    ]


def cells(row: str) -> list[str]:
    return [c.strip() for c in row.split("|") if c.strip()]


def check(matrix: str, audit: str) -> list[str]:
    """Return a list of failure reasons; empty means every assertion holds."""
    fails: list[str] = []
    section = hardening_section(matrix)

    # 1. the section exists at all -- the single cause of all six red rows.
    if not section:
        return [f"COMPAT_MATRIX.md has no '{HEADING}' section"]

    # 2. every required deliverable is named.
    for name in REQUIRED:
        if name not in section:
            fails.append(f"deliverable not classified: {name!r}")

    rows = table_rows(section)

    # 3. at least six classified rows.
    if len(rows) < 6:
        fails.append(f"need >= 6 classified rows, found {len(rows)}")

    # 4. every row's classification column uses an approved label.
    for row in rows:
        cols = cells(row)
        if len(cols) >= 2 and not any(a in cols[1] for a in APPROVED):
            fails.append(f"row {cols[0]!r} has non-approved classification {cols[1]!r}")

    # 5. every `Measured` row cites a test file -- and (stricter than the Rust
    #    test) that file must exist, so a citation cannot be fabricated.
    measured = [r for r in rows if "Measured" in r]
    if not measured:
        fails.append("no Measured rows at all")
    for row in measured:
        if "_test.rs" not in row:
            fails.append(f"Measured row cites no test file: {cells(row)[0]!r}")
            continue
        for tok in row.replace("`", " ").replace("(", " ").replace(")", " ").split():
            if tok.endswith("_test.rs") and not os.path.exists(os.path.join(REPO, tok)):
                fails.append(f"cited test file does not exist on disk: {tok}")

    # 6. the section cites the audit it was derived from.
    if "PHASE9_HARDENING_AUDIT.md" not in section:
        fails.append("section does not cite PHASE9_HARDENING_AUDIT.md")

    # 7. the section separates test-backed artifacts from docs-only claims,
    #    which is the audit's core rule and the one assertion that still failed
    #    after the table landed.
    if "docs-only" not in section and "measured test-backed" not in section:
        fails.append("section does not distinguish measured artifacts from docs-only claims")

    # 7. the audit side (these two were already green; guard against regression).
    for family in ["benchmark", "fuzz", "crash triage", "release train",
                   "onboarding", "migration guide"]:
        if family not in audit.lower():
            fails.append(f"audit does not classify {family!r}")
    if not any(a in audit for a in APPROVED):
        fails.append("audit uses no approved status label")

    return fails


def self_test(matrix: str, audit: str) -> int:
    """Prove the checker catches each failure mode it claims to cover."""
    cases = [
        ("section removed", matrix.replace(HEADING, "## Something Else", 1)),
        (
            "classification label not approved",
            matrix.replace(
                "| Contributor onboarding | Measured |",
                "| Contributor onboarding | Looks fine honestly |",
                1,
            ),
        ),
        (
            "Measured row cites a test file that does not exist",
            matrix.replace(
                "contributor_onboarding_docs_test.rs",
                "a_test_file_that_was_never_written_test.rs",
                1,
            ),
        ),
        ("audit citation removed", matrix.replace("PHASE9_HARDENING_AUDIT.md", "somewhere", 1)),
        (
            "measured-vs-docs-only distinction removed",
            # Both tokens appear more than once, so a count-limited replace
            # leaves one behind and the injection proves nothing.
            matrix.replace("docs-only", "fine").replace("measured test-backed", "fine"),
        ),
    ]
    bad = 0
    for label, mutated in cases:
        if mutated == matrix:
            print(f"  SELF-TEST INCONCLUSIVE: {label} -- injection did not change the text")
            bad += 1
            continue
        if check(mutated, audit):
            print(f"  caught: {label}")
        else:
            print(f"  NOT CAUGHT: {label}")
            bad += 1
    return bad


def main() -> int:
    matrix = open(MATRIX, encoding="utf-8").read()
    audit = open(AUDIT, encoding="utf-8").read()

    if "--self-test" in sys.argv:
        print("self-test: injecting false rows, each must be caught")
        bad = self_test(matrix, audit)
        print("self-test PASSED" if not bad else f"self-test FAILED ({bad} uncaught)")
        return 1 if bad else 0

    fails = check(matrix, audit)
    if fails:
        print(f"FAIL ({len(fails)}):")
        for f in fails:
            print(f"  - {f}")
        return 1
    rows = table_rows(hardening_section(matrix))
    print(f"OK: {len(rows)} classified rows, "
          f"{len([r for r in rows if 'Measured' in r])} Measured and each citing a file that exists")
    return 0


if __name__ == "__main__":
    sys.exit(main())
