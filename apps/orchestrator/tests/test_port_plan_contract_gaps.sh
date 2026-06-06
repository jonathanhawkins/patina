#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "$0")/../../.." && pwd)
ORCH_ROOT=$(cd "$(dirname "$0")/.." && pwd)

python3 - "$ROOT_DIR/prd/PORT_PLAN_CONTRACT_GAPS.json" <<'PY'
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as fh:
    data = json.load(fh)

if not isinstance(data, list) or not data:
    raise SystemExit("contract gaps must be a non-empty JSON list")

seen = set()
for idx, item in enumerate(data, start=1):
    if not isinstance(item, dict):
        raise SystemExit(f"entry {idx} is not an object")
    for field in ("title", "priority", "labels", "description"):
        value = item.get(field)
        if value in (None, ""):
            raise SystemExit(f"entry {idx} missing required field: {field}")
    title = item["title"]
    if title in seen:
        raise SystemExit(f"duplicate title: {title}")
    seen.add(title)

print("port plan contract gaps: ok")
PY
