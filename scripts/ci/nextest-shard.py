#!/usr/bin/env python3
"""Run one shard of the workspace's integration tests under cargo-nextest.

Usage (from engine-rs/):  python3 ../scripts/ci/nextest-shard.py <index> <count>
                          python3 ../scripts/ci/nextest-shard.py --list <index> <count>

Why this exists: every file in a crate's `tests/` folder compiles to its own
test executable that statically links the engine (~100 MB in a debug build).
The workspace has ~1,260 of them, which is far more than a GitHub runner's
disk, so `cargo nextest run --workspace` cannot even finish building in CI.
Each shard builds and runs only its slice of the integration-test targets;
unit tests (`--lib --bins`) and doctests run in the main `Rust` job.

Targets are sorted by (package, name) and dealt out round-robin, so shards stay
balanced and the assignment is stable. Every target lands in exactly one shard
(`--check` verifies that over all shards).
"""

import json
import subprocess
import sys


def integration_targets():
    meta = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"]
        )
    )
    members = set(meta["workspace_members"])
    out = []
    for pkg in meta["packages"]:
        if pkg["id"] not in members:
            continue
        for target in pkg["targets"]:
            if "test" in target["kind"]:
                out.append((pkg["name"], target["name"]))
    return sorted(out)


def shard(targets, index, count):
    return [t for i, t in enumerate(targets) if i % count == index]


def by_package(targets):
    grouped = {}
    for pkg, name in targets:
        grouped.setdefault(pkg, []).append(name)
    return grouped


def main(argv):
    if argv[:1] == ["--check"]:
        count = int(argv[1])
        targets = integration_targets()
        seen = [t for i in range(count) for t in shard(targets, i, count)]
        assert sorted(seen) == targets and len(seen) == len(set(seen)), "shards overlap or miss targets"
        print(f"{len(targets)} integration-test targets, {count} shards, each target exactly once")
        return 0

    list_only = argv[:1] == ["--list"]
    if list_only:
        argv = argv[1:]
    index, count = int(argv[0]), int(argv[1])
    if not 0 <= index < count:
        raise SystemExit(f"shard index {index} out of range for {count} shards")

    mine = shard(integration_targets(), index, count)
    print(f"shard {index + 1}/{count}: {len(mine)} integration-test targets", flush=True)
    if list_only:
        for pkg, name in mine:
            print(f"  {pkg} --test {name}")
        return 0

    failed = []
    # One nextest invocation per package: `--test <name>` is not scoped to a
    # package, so mixing packages could select a same-named target twice.
    for pkg, names in by_package(mine).items():
        cmd = ["cargo", "nextest", "run", "--no-fail-fast", "-p", pkg]
        for name in names:
            cmd += ["--test", name]
        print(f"::group::{pkg} ({len(names)} targets)", flush=True)
        code = subprocess.call(cmd)
        print("::endgroup::", flush=True)
        if code != 0:
            failed.append(pkg)
            print(f"::error::{pkg}: integration tests failed in this shard (exit {code})", flush=True)

    if failed:
        print(f"shard {index + 1}/{count} FAILED in: {', '.join(failed)}")
        return 1
    print(f"shard {index + 1}/{count} passed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
