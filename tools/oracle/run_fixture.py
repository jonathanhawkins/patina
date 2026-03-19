#!/usr/bin/env python3
"""Generate bootstrap oracle goldens from machine-readable probe payloads."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

from common import REPO_ROOT

TOOL_DIR = Path(__file__).resolve().parent

BOOTSTRAP_FIXTURES = {
    "scene_simple_hierarchy_01": {
        "script": "scene_tree_dumper.py",
        "input": REPO_ROOT / "fixtures/oracle_inputs/scene_simple_hierarchy_01.json",
        "output": REPO_ROOT / "fixtures/golden/scenes/scene_simple_hierarchy_01.json",
    },
    "resource_simple_01": {
        "script": "resource_roundtrip.py",
        "input": REPO_ROOT / "fixtures/oracle_inputs/resource_simple_01.json",
        "output": REPO_ROOT / "fixtures/golden/resources/resource_simple_01.json",
    },
}


def run_fixture(fixture_id: str) -> None:
    fixture = BOOTSTRAP_FIXTURES[fixture_id]
    subprocess.run(
        [
            sys.executable,
            str(TOOL_DIR / fixture["script"]),
            "--fixture-id",
            fixture_id,
            "--input",
            str(fixture["input"]),
            "--output",
            str(fixture["output"]),
        ],
        check=True,
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--fixture-id",
        choices=sorted(BOOTSTRAP_FIXTURES.keys()),
        action="append",
        help="Bootstrap fixture(s) to generate. Defaults to all bootstrap fixtures.",
    )
    args = parser.parse_args()

    fixture_ids = args.fixture_id or sorted(BOOTSTRAP_FIXTURES.keys())
    for fixture_id in fixture_ids:
        run_fixture(fixture_id)
        print(f"generated {fixture_id}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
