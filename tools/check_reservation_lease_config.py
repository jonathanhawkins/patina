#!/usr/bin/env python3
"""Verify the Agent Mail hot-file reservation lease config, and that the
2026-07-26 audit triage reports were filed on their epics (pat-m81g6).

Exists as a script rather than a `python3 -c` one-liner on purpose: the
orchestrator's command extractor splits reported commands on `;` and also
scrapes the fenced JSON payload, so a long inline command with embedded quotes
and semicolons reaches the verifier truncated and dies in `bash -c` with
"unexpected EOF". A short, quote-free `python3 <abs-path>` survives intact.

Exit 0 and print OK when everything holds; raise with a named reason otherwise.
"""

import subprocess
import sys

ROOT = "/Users/bone/dev/games/patina"

# An abandoned reservation on a hot shared file used to be held for
# inactivity(1800) + grace(900) = 45 minutes. These cut that to 15. Both
# reclaim on INACTIVITY, so an agent still touching its files renews and a
# genuinely long edit keeps its lock.
EXPECTED_ENV = [
    "FILE_RESERVATION_INACTIVITY_SECONDS=600",
    "FILE_RESERVATION_ACTIVITY_GRACE_SECONDS=300",
]

# (epic, substring that must appear in one of its comments)
EXPECTED_COMMENTS = [
    ("pat-0q2kx", "TRIAGE SWEEP 2026-07-26"),
    ("pat-0q2kx", "CLOSE CANDIDATES"),
    ("pat-nzgdu", "this epic is effectively COMPLETE"),
]


def env_text() -> str:
    with open(f"{ROOT}/mcp_agent_mail/.env", encoding="utf-8") as fh:
        return fh.read()


def comments(bead: str) -> str:
    # cwd is pinned: `br` auto-discovers .beads/*.db from the working directory,
    # and the verifier does not run from the repo root.
    proc = subprocess.run(
        ["br", "comments", "list", bead, "--no-auto-import", "--allow-stale"],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    return proc.stdout


def main() -> int:
    text = env_text()
    for setting in EXPECTED_ENV:
        if setting not in text:
            raise SystemExit(f"FAIL: {setting} missing from mcp_agent_mail/.env")

    cache: dict[str, str] = {}
    for bead, needle in EXPECTED_COMMENTS:
        if bead not in cache:
            cache[bead] = comments(bead)
        if needle not in cache[bead]:
            raise SystemExit(f"FAIL: {bead} has no comment containing {needle!r}")

    print("OK: reservation lease TTLs shortened + triage reports present on both epics")
    return 0


if __name__ == "__main__":
    sys.exit(main())
