#!/usr/bin/env bash
# refresh_api.sh — Reproducible API extraction from pinned upstream Godot.
#
# This is the single entry point for refreshing all extracted API artifacts
# used by Patina's parity checks. It orchestrates:
#
#   1. GDExtension probe extraction (classdb, node_defaults, resource metadata, etc.)
#   2. Oracle fixture capture (scene trees, properties, signals per .tscn)
#   3. Artifact installation into fixtures/oracle_outputs/
#
# Usage:
#   ./scripts/refresh_api.sh [--probes-only] [--oracle-only] [--dry-run]
#
# Environment:
#   PATINA_GODOT      — path to Godot binary (required, or auto-detected)
#   PATINA_SKIP_BUILD — set to 1 to skip cargo build steps
#
# The pinned Godot version is defined in tools/oracle/common.py.
# This script validates the detected Godot version against that pin.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# --- Parse flags ---
RUN_PROBES=1
RUN_ORACLE=1
DRY_RUN=0

for arg in "$@"; do
    case "$arg" in
        --probes-only)  RUN_ORACLE=0 ;;
        --oracle-only)  RUN_PROBES=0 ;;
        --dry-run)      DRY_RUN=1 ;;
        --help|-h)
            echo "Usage: $0 [--probes-only] [--oracle-only] [--dry-run]"
            echo ""
            echo "Refreshes extracted API artifacts from pinned upstream Godot."
            echo ""
            echo "Flags:"
            echo "  --probes-only   Run GDExtension probes only (skip oracle fixtures)"
            echo "  --oracle-only   Run oracle fixtures only (skip GDExtension probes)"
            echo "  --dry-run       Validate setup without running extraction"
            echo ""
            echo "Environment:"
            echo "  PATINA_GODOT      Path to Godot binary"
            echo "  PATINA_SKIP_BUILD Set to 1 to skip cargo build"
            exit 0
            ;;
        *)
            echo "Unknown flag: $arg (use --help)"
            exit 1
            ;;
    esac
done

# --- Locate Godot binary ---
GODOT="${PATINA_GODOT:-}"

if [ -z "$GODOT" ]; then
    # Try platform-specific locations, then PATH
    CANDIDATES=()

    case "$(uname -s)" in
        Darwin)
            # macOS: check common .app bundle locations
            for app_dir in "$HOME/Downloads" "$HOME/Applications" "/Applications"; do
                if [ -x "$app_dir/Godot.app/Contents/MacOS/Godot" ]; then
                    CANDIDATES+=("$app_dir/Godot.app/Contents/MacOS/Godot")
                fi
            done
            ;;
        Linux)
            # Linux: check common install paths
            for bin_dir in "$HOME/.local/bin" "/usr/local/bin" "/usr/bin"; do
                if [ -x "$bin_dir/godot" ]; then
                    CANDIDATES+=("$bin_dir/godot")
                fi
            done
            ;;
    esac

    # Always check PATH as fallback
    PATH_GODOT="$(command -v godot 2>/dev/null || true)"
    if [ -n "$PATH_GODOT" ]; then
        CANDIDATES+=("$PATH_GODOT")
    fi

    for candidate in "${CANDIDATES[@]}"; do
        if [ -n "$candidate" ] && [ -x "$candidate" ]; then
            GODOT="$candidate"
            break
        fi
    done
fi

if [ -z "$GODOT" ] || [ ! -x "$GODOT" ]; then
    echo "ERROR: Godot binary not found."
    echo "  Set PATINA_GODOT=/path/to/godot or install godot in PATH."
    exit 1
fi

# --- Read pinned version ---
PINNED_VERSION=""
PINNED_COMMIT=""
if [ -f "$REPO_ROOT/tools/oracle/common.py" ]; then
    PINNED_VERSION=$(grep 'UPSTREAM_VERSION' "$REPO_ROOT/tools/oracle/common.py" | head -1 | sed 's/.*"\(.*\)".*/\1/')
    PINNED_COMMIT=$(grep 'UPSTREAM_COMMIT' "$REPO_ROOT/tools/oracle/common.py" | head -1 | sed 's/.*"\(.*\)".*/\1/')
fi

# --- Validate upstream submodule pin ---
UPSTREAM_DIR="$REPO_ROOT/upstream/godot"
if [ -d "$UPSTREAM_DIR/.git" ] || [ -f "$UPSTREAM_DIR/.git" ]; then
    SUBMODULE_COMMIT=$(cd "$UPSTREAM_DIR" && git rev-parse HEAD 2>/dev/null || echo "unknown")
    if [ -n "$PINNED_COMMIT" ] && [ "$SUBMODULE_COMMIT" != "unknown" ]; then
        if [ "$SUBMODULE_COMMIT" = "$PINNED_COMMIT" ]; then
            echo "[OK] upstream/godot submodule matches pinned commit"
        else
            echo "WARNING: upstream/godot submodule commit mismatch!"
            echo "  Pinned (common.py):  $PINNED_COMMIT"
            echo "  Submodule HEAD:      $SUBMODULE_COMMIT"
            echo ""
            echo "  Run: cd upstream/godot && git checkout $PINNED_COMMIT"
            echo ""
        fi
    fi
else
    echo "NOTE: upstream/godot submodule not checked out (optional for probe extraction)"
fi

# --- Detect actual Godot version ---
GODOT_VERSION=$("$GODOT" --version 2>/dev/null | head -1 | tr -d '\n' || echo "unknown")

echo "=============================================="
echo "  Patina API Extraction"
echo "=============================================="
echo "  Godot binary:    $GODOT"
echo "  Godot version:   $GODOT_VERSION"
echo "  Pinned version:  ${PINNED_VERSION:-not set}"
echo "  Repo root:       $REPO_ROOT"
echo "  Run probes:      $([ "$RUN_PROBES" -eq 1 ] && echo yes || echo no)"
echo "  Run oracle:      $([ "$RUN_ORACLE" -eq 1 ] && echo yes || echo no)"
echo "  Dry run:         $([ "$DRY_RUN" -eq 1 ] && echo yes || echo no)"
echo "=============================================="
echo ""

# --- Version pin validation ---
if [ -n "$PINNED_VERSION" ]; then
    # Check if actual version contains the pinned major.minor.patch
    PINNED_SHORT=$(echo "$PINNED_VERSION" | sed 's/-.*//')
    if echo "$GODOT_VERSION" | grep -q "$PINNED_SHORT"; then
        echo "[OK] Godot version matches pin ($PINNED_VERSION)"
    else
        echo "WARNING: Godot version mismatch!"
        echo "  Pinned:  $PINNED_VERSION"
        echo "  Actual:  $GODOT_VERSION"
        echo ""
        echo "  Artifacts will be stamped with actual version."
        echo "  Update tools/oracle/common.py if intentional."
        echo ""
    fi
fi

if [ "$DRY_RUN" -eq 1 ]; then
    echo ""
    echo "[DRY RUN] Setup validated. No extraction performed."
    exit 0
fi

# --- Timestamps ---
START_TS=$(date -u +%Y-%m-%dT%H:%M:%SZ)
PROBE_OUTPUT="$REPO_ROOT/apps/godot/probe_output"
ORACLE_OUTPUT="$REPO_ROOT/fixtures/oracle_outputs"
FIXTURE_PROJECT="$REPO_ROOT/fixtures/sample_project"

PROBES_OK=0
ORACLE_OK=0

# --- Phase 1: GDExtension Probes ---
if [ "$RUN_PROBES" -eq 1 ]; then
    echo ""
    echo "====== Phase 1: GDExtension Probes ======"
    echo ""

    if "$REPO_ROOT/apps/godot/extract_probes.sh" "$GODOT" "$PROBE_OUTPUT"; then
        PROBES_OK=1

        # Install classdb signatures into fixtures
        if [ -f "$PROBE_OUTPUT/classdb_probe_signatures.json" ]; then
            cp "$PROBE_OUTPUT/classdb_probe_signatures.json" "$ORACLE_OUTPUT/classdb_probe_signatures.json"
            echo ""
            echo "[INSTALLED] classdb_probe_signatures.json -> fixtures/oracle_outputs/"
        fi
    else
        echo ""
        echo "ERROR: Probe extraction failed."
    fi
fi

# --- Phase 2: Oracle Fixture Capture ---
if [ "$RUN_ORACLE" -eq 1 ]; then
    echo ""
    echo "====== Phase 2: Oracle Fixture Capture ======"
    echo ""

    if [ ! -d "$FIXTURE_PROJECT" ] || [ ! -f "$FIXTURE_PROJECT/project.godot" ]; then
        echo "WARNING: Sample project not found at $FIXTURE_PROJECT"
        echo "  Skipping oracle fixture capture."
    elif [ -f "$REPO_ROOT/tools/oracle/run_all.sh" ]; then
        if GODOT="$GODOT" "$REPO_ROOT/tools/oracle/run_all.sh" "$FIXTURE_PROJECT" "$ORACLE_OUTPUT"; then
            ORACLE_OK=1
        else
            echo ""
            echo "ERROR: Oracle capture failed."
        fi
    else
        echo "WARNING: tools/oracle/run_all.sh not found — skipping oracle capture."
    fi
fi

# --- Summary ---
END_TS=$(date -u +%Y-%m-%dT%H:%M:%SZ)

echo ""
echo "=============================================="
echo "  Extraction Summary"
echo "=============================================="
echo "  Started:       $START_TS"
echo "  Finished:      $END_TS"
echo "  Godot version: $GODOT_VERSION"
echo "  Probes:        $([ "$RUN_PROBES" -eq 0 ] && echo skipped || ([ "$PROBES_OK" -eq 1 ] && echo PASS || echo FAIL))"
echo "  Oracle:        $([ "$RUN_ORACLE" -eq 0 ] && echo skipped || ([ "$ORACLE_OK" -eq 1 ] && echo PASS || echo FAIL))"
echo ""

if [ "$RUN_PROBES" -eq 1 ] && [ -f "$PROBE_OUTPUT/manifest.json" ]; then
    echo "  Probe output:  $PROBE_OUTPUT/"
    echo "  Manifest:      $PROBE_OUTPUT/manifest.json"
fi

if [ -f "$ORACLE_OUTPUT/classdb_probe_signatures.json" ]; then
    echo "  Signatures:    $ORACLE_OUTPUT/classdb_probe_signatures.json"
fi

echo ""
echo "To verify parity after refresh:"
echo "  cd engine-rs && cargo test --workspace"
echo "=============================================="

# Exit with error if any requested phase failed
if [ "$RUN_PROBES" -eq 1 ] && [ "$PROBES_OK" -eq 0 ]; then
    exit 1
fi
if [ "$RUN_ORACLE" -eq 1 ] && [ "$ORACLE_OK" -eq 0 ]; then
    exit 1
fi
