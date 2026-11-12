#!/usr/bin/env python3
"""Shared helpers for Patina oracle tooling."""

from __future__ import annotations

import json
import re
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
UPSTREAM_VERSION = "4.6.1-stable"
UPSTREAM_COMMIT = "14d19694e0c88a3f9e82d899a0400f27a24c176e"


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2)
        handle.write("\n")


def timestamp_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


def make_envelope(fixture_id: str, capture_type: str, data: Any) -> dict[str, Any]:
    return {
        "fixture_id": fixture_id,
        "upstream_version": UPSTREAM_VERSION,
        "upstream_commit": UPSTREAM_COMMIT,
        "capture_type": capture_type,
        "generated_at": timestamp_now(),
        "data": data,
    }


_VECTOR2_RE = re.compile(
    r"^Vector2\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)$"
)


def parse_variant(value: Any) -> dict[str, Any]:
    if isinstance(value, dict) and {"type", "value"} <= value.keys():
        return value
    if isinstance(value, bool):
        return {"type": "Bool", "value": value}
    if isinstance(value, int):
        return {"type": "Int", "value": value}
    if isinstance(value, float):
        return {"type": "Float", "value": value}
    if isinstance(value, list):
        return {"type": "Array", "value": value}
    if value is None:
        return {"type": "Nil", "value": None}
    if not isinstance(value, str):
        return {"type": "String", "value": str(value)}

    text = value.strip()
    if text == "true":
        return {"type": "Bool", "value": True}
    if text == "false":
        return {"type": "Bool", "value": False}
    if re.fullmatch(r"-?\d+", text):
        return {"type": "Int", "value": int(text)}
    if re.fullmatch(r"-?\d+(?:\.\d+)?", text):
        return {"type": "Float", "value": float(text)}
    match = _VECTOR2_RE.fullmatch(text)
    if match:
        return {
            "type": "Vector2",
            "value": [float(match.group(1)), float(match.group(2))],
        }
    return {"type": "String", "value": text}


def normalize_scene_tree(payload: Any) -> dict[str, Any]:
    def normalize_node(node: dict[str, Any]) -> dict[str, Any]:
        return {
            "name": node.get("name", ""),
            "class": node.get("class", ""),
            "path": node.get("path", ""),
            "children": [normalize_node(child) for child in node.get("children", [])],
            "properties": {
                key: parse_variant(value)
                for key, value in node.get("properties", {}).items()
            },
        }

    if isinstance(payload, dict) and "nodes" in payload:
        return {"nodes": [normalize_node(node) for node in payload["nodes"]]}
    if isinstance(payload, dict) and "name" in payload:
        return {"nodes": [normalize_node(payload)]}
    raise ValueError("scene payload must contain a root node or a nodes array")


def normalize_resource(payload: Any) -> dict[str, Any]:
    return {
        "class_name": payload.get("class_name", "Resource"),
        "properties": {
            key: parse_variant(value)
            for key, value in payload.get("properties", {}).items()
        },
        "subresources": payload.get("subresources", {}),
    }
