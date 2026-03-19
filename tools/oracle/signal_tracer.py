#!/usr/bin/env python3
"""Wrap signal trace output in the Patina oracle envelope."""

from __future__ import annotations

import argparse
from pathlib import Path

from common import load_json, make_envelope, write_json


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture-id", required=True)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    payload = load_json(args.input)
    write_json(args.output, make_envelope(args.fixture_id, "signals", payload))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
