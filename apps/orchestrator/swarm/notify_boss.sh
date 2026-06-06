#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "$0")/.." && pwd)/orch_env.sh"

SESSION_NAME="${1:?usage: notify_boss.sh <session> <message...>}"
shift
MESSAGE="${*:-No message provided}"
NOTICE_LOG="${PROJECT_ROOT}/.codex/orchestrator-notices.log"
TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')

mkdir -p "$(dirname "$NOTICE_LOG")"
printf '[%s] %s\n' "$TIMESTAMP" "$MESSAGE" >> "$NOTICE_LOG"

if tmux list-panes -t "${SESSION_NAME}:0" -F '#{pane_index}|#{pane_dead}' 2>/dev/null \
  | grep -q '^0|0$'; then
  tmux display-message -d 8000 -t "${SESSION_NAME}:0.0" \
    "[orchestrator ${TIMESTAMP}] ${MESSAGE} (see .codex/orchestrator-notices.log)"
fi
