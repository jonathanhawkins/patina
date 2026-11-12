#!/usr/bin/env python3
"""Capture headless-Godot conformance goldens for pat-mgixk.403.

Runs each `engine-rs/fixtures/conformance/<name>/` project in real headless
Godot and writes the resulting trace envelope to
`engine-rs/fixtures/golden/conformance/<name>.json`.

The envelope is the contract the Patina side has to reproduce:

    {
      "project": "<name>",
      "frames": <int>,
      "prints": ["[T] ...", ...],        # ordered, fixture-prefixed stdout
      "signals": [{"signal":..., "source":..., "frame":...}, ...],
      "tree":    [{"path":..., "class":..., "properties": {...}}, ...]
    }

The fixture project is never modified. Each run copies the project into a
temporary directory, drops `tools/oracle/conformance_probe.gd` in beside it and
generates an `override.cfg` that registers the probe as an autoload, so the
probe is in the tree before the main scene the way Godot boots a real autoload.

This script is a BLESS step: it is run by hand when a golden is first created or
deliberately re-recorded, never as part of the test suite.

Usage:
    python3 tools/oracle/conformance_capture.py                    # all projects
    python3 tools/oracle/conformance_capture.py timer_queue_free   # one project
    python3 tools/oracle/conformance_capture.py --check            # no writes
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
PROJECTS_DIR = REPO_ROOT / "engine-rs" / "fixtures" / "conformance"
GOLDEN_DIR = REPO_ROOT / "engine-rs" / "fixtures" / "golden" / "conformance"
PROBE = Path(__file__).resolve().parent / "conformance_probe.gd"

#: Marker every fixture prefixes its observable output with. Godot offers a
#: script no hook on `print()`, so the harness reads the lines off stdout —
#: which also carries engine warnings, driver chatter and the probe's own
#: message. The prefix is what separates the fixture's output from all of it.
PRINT_MARKER = "[T] "

#: Frames to run. 12 at a fixed 60 fps is 0.2 s — long enough for a 0.1 s Timer
#: to fire and for a deferred call to land, short enough to stay deterministic.
DEFAULT_FRAMES = 12

GODOT_CANDIDATES = [
    "/Applications/Godot.app/Contents/MacOS/Godot",
    "/usr/local/bin/godot",
    "/usr/bin/godot",
]


def find_godot() -> str:
    """Locates the Godot binary, preferring `$GODOT_BIN`."""
    env = os.environ.get("GODOT_BIN")
    if env and Path(env).exists():
        return env
    for candidate in GODOT_CANDIDATES:
        if Path(candidate).exists():
            return candidate
    found = shutil.which("godot")
    if found:
        return found
    raise SystemExit(
        "Godot not found. Set GODOT_BIN to the Godot 4.6 binary, e.g.\n"
        "  GODOT_BIN=/Applications/Godot.app/Contents/MacOS/Godot "
        "python3 tools/oracle/conformance_capture.py"
    )


def project_names() -> list[str]:
    if not PROJECTS_DIR.is_dir():
        return []
    return sorted(
        p.name for p in PROJECTS_DIR.iterdir()
        if (p / "project.godot").is_file()
    )


def capture(godot: str, name: str, frames: int) -> dict:
    """Runs one project in headless Godot and returns its trace envelope."""
    source = PROJECTS_DIR / name
    with tempfile.TemporaryDirectory(prefix=f"conformance_{name}_") as tmp:
        work = Path(tmp) / "project"
        shutil.copytree(source, work)

        # The probe lives beside the project's own files ONLY in the copy, and
        # is registered through override.cfg rather than by editing
        # project.godot — the fixture on disk stays byte-identical.
        shutil.copyfile(PROBE, work / "__conformance_probe.gd")
        (work / "override.cfg").write_text(
            "[autoload]\n\n"
            'ConformanceProbe="*res://__conformance_probe.gd"\n',
            encoding="utf-8",
        )

        out_json = Path(tmp) / "envelope.json"
        cmd = [
            godot,
            "--headless",
            "--path", str(work),
            "--fixed-fps", "60",
            "--",
            "--output", str(out_json),
            "--frames", str(frames),
        ]
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=120, check=False
        )

        if not out_json.is_file():
            sys.stderr.write(
                f"--- {name}: probe wrote no envelope (exit {proc.returncode}) ---\n"
                f"{proc.stdout}\n{proc.stderr}\n"
            )
            raise SystemExit(f"{name}: capture failed")

        envelope = json.loads(out_json.read_text(encoding="utf-8"))

    prints = [
        line[len(PRINT_MARKER):]
        for line in proc.stdout.splitlines()
        if line.startswith(PRINT_MARKER)
    ]

    return {
        "project": name,
        "frames": envelope.get("frames", frames),
        "prints": prints,
        "signals": envelope.get("signals", []),
        "tree": envelope.get("tree", []),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("projects", nargs="*", help="project names (default: all)")
    parser.add_argument("--frames", type=int, default=DEFAULT_FRAMES)
    parser.add_argument(
        "--check",
        action="store_true",
        help="capture and compare against the checked-in goldens without writing",
    )
    args = parser.parse_args()

    godot = find_godot()
    names = args.projects or project_names()
    if not names:
        raise SystemExit(f"no conformance projects under {PROJECTS_DIR}")

    GOLDEN_DIR.mkdir(parents=True, exist_ok=True)
    drifted = []
    for name in names:
        envelope = capture(godot, name, args.frames)
        text = json.dumps(envelope, indent=2, sort_keys=True) + "\n"
        target = GOLDEN_DIR / f"{name}.json"
        if args.check:
            current = target.read_text(encoding="utf-8") if target.is_file() else ""
            status = "same" if current == text else "DRIFT"
            if status == "DRIFT":
                drifted.append(name)
            print(f"{status:>5}  {name}")
        else:
            target.write_text(text, encoding="utf-8")
            print(
                f"wrote {target.relative_to(REPO_ROOT)} "
                f"({len(envelope['prints'])} prints, "
                f"{len(envelope['signals'])} signals, "
                f"{len(envelope['tree'])} nodes)"
            )

    if drifted:
        sys.stderr.write(
            "goldens differ from a fresh Godot capture: " + ", ".join(drifted) + "\n"
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
