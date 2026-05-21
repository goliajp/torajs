#!/usr/bin/env bash
#
# hardev autorun pillar — Claude Code `Stop` hook handler.
#
# Wired in via `.claude/settings.local.json`:
#
#   "hooks": {
#     "Stop": [
#       { "hooks": [
#           { "type": "command", "command": "hardev/autorun/stop_hook.sh" }
#         ]
#       }
#     ]
#   }
#
# Claude Code invokes this script when an agent turn ends. CWD is the
# project root.
#
# Sentinel lifecycle:
#
#   trigger.sh  →  writes .claude/autorun-intent (rotation_id, single line)
#                  this hook reads it
#   stop_hook   →  on green INV check, writes .claude/autorun-marker
#                  (same rotation_id) and rm's the intent
#                  on red, keeps intent so the next turn-end retries
#   watcherd    →  fswatch the marker; on appearance, tmux send-keys
#                  /clear + /handoff:handoff resume, then rm marker
#
# Each sentinel is consumed exactly once on the green path. On the red
# path, intent is kept (agent may fix the failed invariant and try
# again next turn-end without re-running trigger.sh).
#
# Always exits 0: a hook failure must never break the user's turn.

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
. "$SCRIPT_DIR/lib.sh"

# No intent ⇒ this is just a normal turn-end. Nothing to do.
if [ ! -f "$INTENT_FILE" ]; then
  exit 0
fi

rid=$(head -1 "$INTENT_FILE" 2>/dev/null | tr -d '[:space:]')
if [ -z "$rid" ]; then
  echo "stop_hook: $INTENT_FILE is empty, ignoring" >&2
  exit 0
fi

# Run the INV-1..5 pre-act gate.
if "$SCRIPT_DIR/check.sh" "$rid" >&2; then
  # Green: hand off to the watcher.
  printf '%s\n' "$rid" > "$MARKER_FILE"
  rm -f "$INTENT_FILE"
  echo "stop_hook: rotation $rid green-lit · marker $MARKER_FILE written · intent consumed" >&2
else
  # Red: leave intent in place so the agent can retry next turn-end
  # after fixing the failed INV (typically: re-save handoff, commit
  # working tree).
  echo "stop_hook: rotation $rid blocked by INV check · intent kept · marker NOT written" >&2
fi

exit 0
