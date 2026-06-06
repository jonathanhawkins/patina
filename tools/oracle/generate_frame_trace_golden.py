#!/usr/bin/env python3
"""Generate an upstream frame-trace golden for test_scripts from Godot's behavioral contract.

This script produces the expected event trace that upstream Godot 4.x would emit
when running the test_scripts.tscn scene for N frames, based on:

1. The oracle scene tree (fixtures/oracle_outputs/test_scripts.json)
2. The GDScript source files (fixtures/scripts/*.gd)
3. Godot's documented notification ordering contract:
   - ENTER_TREE: top-down (parent before children)
   - READY: bottom-up (children before parent), with _ready script calls
   - Per-frame processing order (tree order, only for nodes with processing enabled):
     a. INTERNAL_PHYSICS_PROCESS (only nodes with set_physics_process_internal(true))
     b. PHYSICS_PROCESS (only nodes with _physics_process override or set_physics_process(true))
     c. INTERNAL_PROCESS (only nodes with set_process_internal(true))
     d. PROCESS (only nodes with _process override or set_process(true))
     e. Script _process/_physics_process calls bracketed with call/return

Usage:
    python generate_frame_trace_golden.py --oracle <oracle_json> --scripts-dir <dir> --frames 10 --output <path>
"""

from __future__ import annotations

import argparse
import json
import os
import re
from pathlib import Path


def parse_script_methods(script_path: Path) -> set[str]:
    """Extract overridden method names from a GDScript file."""
    methods = set()
    if not script_path.exists():
        return methods
    text = script_path.read_text()
    for match in re.finditer(r"^func\s+(_\w+)\s*\(", text, re.MULTILINE):
        methods.add(match.group(1))
    return methods


def collect_nodes(tree: dict, scripts_dir: Path) -> list[dict]:
    """Walk the oracle scene tree and collect node metadata in tree order."""
    nodes = []

    def walk(node: dict) -> None:
        path = node.get("path", "")
        cls = node.get("class", "Node")
        script_res = node.get("script", "")

        # Resolve script path
        script_methods: set[str] = set()
        if script_res:
            # Convert res://scripts/foo.gd -> scripts_dir/foo.gd
            rel = script_res.replace("res://", "")
            script_methods = parse_script_methods(scripts_dir / rel)

        # Determine processing flags based on Godot 4.x defaults
        is_window = cls in ("Window", "Viewport", "SubViewport")
        has_process = "_process" in script_methods
        has_physics_process = "_physics_process" in script_methods
        has_ready = "_ready" in script_methods

        nodes.append(
            {
                "path": path,
                "class": cls,
                "script": script_res,
                "has_process": has_process,
                "has_physics_process": has_physics_process,
                "has_ready": has_ready,
                "has_script": bool(script_res),
                # Window/Viewport nodes have internal processing in Godot
                "internal_physics_process": is_window,
                "internal_process": is_window,
            }
        )
        for child in node.get("children", []):
            walk(child)

    walk(tree)
    return nodes


def generate_lifecycle_events(nodes: list[dict]) -> list[dict]:
    """Generate frame-0 lifecycle events: ENTER_TREE (top-down) and READY (bottom-up)."""
    events = []

    # Skip root Window for ENTER_TREE — the capture script attaches probes
    # after scene load, and root is already in the tree. Only scene children
    # get ENTER_TREE in the capture.
    scene_nodes = [n for n in nodes if n["path"] != "/root"]

    # ENTER_TREE: top-down (tree order = already correct)
    for node in scene_nodes:
        events.append(
            {
                "event_type": "notification",
                "node_path": node["path"],
                "detail": "ENTER_TREE",
                "frame": 0,
            }
        )

    # READY: bottom-up (reverse tree order, but children before their parent)
    # In Godot, READY fires leaf-first, then parent.
    ready_order = list(reversed(scene_nodes))
    for node in ready_order:
        events.append(
            {
                "event_type": "notification",
                "node_path": node["path"],
                "detail": "READY",
                "frame": 0,
            }
        )
        # If node has _ready in script, add script call/return
        if node["has_ready"]:
            events.append(
                {
                    "event_type": "script_call",
                    "node_path": node["path"],
                    "detail": "_ready",
                    "frame": 0,
                }
            )
            events.append(
                {
                    "event_type": "script_return",
                    "node_path": node["path"],
                    "detail": "_ready",
                    "frame": 0,
                }
            )

    return events


def generate_frame_events(nodes: list[dict], frame: int) -> list[dict]:
    """Generate per-frame processing events based on Godot's notification contract."""
    events = []

    # Phase 1: INTERNAL_PHYSICS_PROCESS (tree order, only for nodes with it enabled)
    for node in nodes:
        if node["internal_physics_process"]:
            events.append(
                {
                    "event_type": "notification",
                    "node_path": node["path"],
                    "detail": "INTERNAL_PHYSICS_PROCESS",
                    "frame": frame,
                }
            )

    # Phase 2: PHYSICS_PROCESS (tree order, only for nodes with _physics_process)
    for node in nodes:
        if node["has_physics_process"]:
            events.append(
                {
                    "event_type": "notification",
                    "node_path": node["path"],
                    "detail": "PHYSICS_PROCESS",
                    "frame": frame,
                }
            )
            if node["has_script"]:
                events.append(
                    {
                        "event_type": "script_call",
                        "node_path": node["path"],
                        "detail": "_physics_process",
                        "frame": frame,
                    }
                )
                events.append(
                    {
                        "event_type": "script_return",
                        "node_path": node["path"],
                        "detail": "_physics_process",
                        "frame": frame,
                    }
                )

    # Phase 3: INTERNAL_PROCESS (tree order, only for nodes with it enabled)
    for node in nodes:
        if node["internal_process"]:
            events.append(
                {
                    "event_type": "notification",
                    "node_path": node["path"],
                    "detail": "INTERNAL_PROCESS",
                    "frame": frame,
                }
            )

    # Phase 4: PROCESS (tree order, only for nodes with _process)
    for node in nodes:
        if node["has_process"]:
            events.append(
                {
                    "event_type": "notification",
                    "node_path": node["path"],
                    "detail": "PROCESS",
                    "frame": frame,
                }
            )
            if node["has_script"]:
                events.append(
                    {
                        "event_type": "script_call",
                        "node_path": node["path"],
                        "detail": "_process",
                        "frame": frame,
                    }
                )
                events.append(
                    {
                        "event_type": "script_return",
                        "node_path": node["path"],
                        "detail": "_process",
                        "frame": frame,
                    }
                )

    return events


def build_tree_snapshot(nodes: list[dict]) -> dict:
    """Build a tree snapshot structure from node metadata."""
    # Build a nested tree from the flat node list
    root_node = None
    path_to_entry: dict[str, dict] = {}

    for node in nodes:
        entry = {
            "class": node["class"],
            "name": node["path"].rsplit("/", 1)[-1] if "/" in node["path"] else node["path"],
            "notifications": [],
            "children": [],
        }
        if node["has_script"]:
            entry["script"] = node["script"]
        path_to_entry[node["path"]] = entry

        if root_node is None:
            root_node = entry
        else:
            # Find parent path
            parent_path = node["path"].rsplit("/", 1)[0]
            if parent_path in path_to_entry:
                path_to_entry[parent_path]["children"].append(entry)

    return root_node or {}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--oracle",
        type=Path,
        required=True,
        help="Path to oracle output JSON (e.g., fixtures/oracle_outputs/test_scripts.json)",
    )
    parser.add_argument(
        "--scripts-dir",
        type=Path,
        required=True,
        help="Path to fixtures directory containing scripts/ subdirectory",
    )
    parser.add_argument(
        "--frames",
        type=int,
        default=10,
        help="Number of frames to generate (default: 10)",
    )
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Output path for the golden JSON file",
    )
    args = parser.parse_args()

    # Load oracle data
    oracle = json.loads(args.oracle.read_text())
    scene_tree = oracle["scene_tree"]

    # Collect node metadata
    nodes = collect_nodes(scene_tree, args.scripts_dir)

    # Generate event trace
    all_events: list[dict] = []

    # Frame 0: lifecycle + processing
    all_events.extend(generate_lifecycle_events(nodes))
    all_events.extend(generate_frame_events(nodes, 0))

    # Frames 1..N-1: processing only
    for frame in range(1, args.frames):
        all_events.extend(generate_frame_events(nodes, frame))

    # Build output in the golden trace format
    golden = {
        "scene_file": f"fixtures/scenes/test_scripts.tscn",
        "frame_count": args.frames,
        "process_time": args.frames / 60.0,
        "physics_time": args.frames / 60.0,
        "event_trace": all_events,
        "tree": build_tree_snapshot(nodes),
        "upstream_version": oracle.get("upstream_version", "unknown"),
        "generated_by": "generate_frame_trace_golden.py",
        "source": "oracle + godot behavioral contract",
    }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(golden, indent=2) + "\n")
    print(f"Generated {len(all_events)} events across {args.frames} frames -> {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
