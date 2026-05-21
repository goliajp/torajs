#!/usr/bin/env bash
#
# hardev autorun pillar — watcher single-shot action.
#
# Run by launchd (P1.4) every time `.claude/autorun-marker` is
# created/modified, via launchd `WatchPaths`. Each invocation is a
# single-shot: examine the marker, run defense-in-depth INV check,
# tmux-send `/clear` and `/handoff:handoff resume` to the target
# pane, consume the marker.
#
# Usage:
#   hardev/autorun/watcherd.sh [--dry-run | --apply]
#
# Modes:
#   --dry-run (default)   log the would-be tmux send-keys without
#                         actually sending — safe for acceptance
#                         test, dev iteration, and first-time setup.
#                         Marker is still consumed.
#   --apply               actually send-keys. Only enable once you
#                         trust the pipeline (P1.5 dogfood).
#
# Required (for --apply, optional for --dry-run):
#   HARDEV_AUTORUN_TMUX_TARGET   tmux target identifier, e.g. `%0`,
#                                `session:window.pane`, or `=Claude`
#                                (per `man tmux` TARGET-PANE).
#                                If unset, watcherd attempts to
#                                discover a pane whose current
#                                command or title contains "claude"
#                                or "node" (Claude Code TUI runs
#                                under node).
#
# Exit codes:
#   0  — marker absent, or rotation cleanly consumed
#   1  — soft failure: marker was present but couldn't be acted on
#         (RED INV check, no target, malformed marker) — marker
#         still rm'd to avoid launchd respawn loop
#   2  — usage error
#
# Discipline: this script is the **only** writer of tmux send-keys
# in the autorun pillar. The Stop hook (P1.2) writes marker only;
# this script reads marker and acts. Two-stage gate: INV-1..5 ran
# at Stop hook time + INV-1..5 re-runs here (defense-in-depth
# against state drift between stop_hook and act).

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
. "$SCRIPT_DIR/lib.sh"

DRY_RUN=true
for arg in "$@"; do
  case "$arg" in
    --apply) DRY_RUN=false ;;
    --dry-run) DRY_RUN=true ;;
    -h|--help)
      sed -n '3,40p' "$0"
      exit 0
      ;;
    *)
      echo "watcherd: unknown arg '$arg'" >&2
      exit 2
      ;;
  esac
done

# 1. Marker present?
if [ ! -f "$MARKER_FILE" ]; then
  exit 0
fi

rid=$(head -1 "$MARKER_FILE" 2>/dev/null | tr -d '[:space:]')
if [ -z "$rid" ]; then
  echo "watcherd: $MARKER_FILE is empty · dropping" >&2
  rm -f "$MARKER_FILE"
  exit 1
fi

# 2. Defense-in-depth INV re-check. State may have changed between
#    stop_hook writing the marker and now (typically: dev introduced
#    dirty tree in the gap).
if ! "$SCRIPT_DIR/check.sh" "$rid" >&2; then
  echo "watcherd: rotation $rid blocked at watcher gate · INV regressed since stop_hook · marker dropped" >&2
  rm -f "$MARKER_FILE"
  exit 1
fi

# 3. Target pane discovery.
target="${HARDEV_AUTORUN_TMUX_TARGET:-}"
if [ -z "$target" ] && command -v tmux >/dev/null 2>&1; then
  # Find a pane whose current command or title contains "claude" or
  # "node" (Claude Code TUI runs under node). Prefer "claude" match
  # over generic "node".
  target=$(tmux list-panes -a -F '#{pane_id} #{pane_current_command} #{pane_title}' 2>/dev/null \
    | grep -Ei '\bclaude\b' | head -1 | awk '{print $1}')
  if [ -z "$target" ]; then
    target=$(tmux list-panes -a -F '#{pane_id} #{pane_current_command} #{pane_title}' 2>/dev/null \
      | grep -Ei '\bnode\b' | head -1 | awk '{print $1}')
  fi
fi
if [ -z "$target" ]; then
  echo "watcherd: no tmux target (HARDEV_AUTORUN_TMUX_TARGET unset and discovery found no claude/node pane) · marker dropped" >&2
  rm -f "$MARKER_FILE"
  exit 1
fi

# 4. Send-keys (or log under dry-run).
if [ "$DRY_RUN" = true ]; then
  echo "watcherd: [DRY-RUN] rotation $rid · would send-keys '/clear' + '/handoff:handoff resume' to tmux target '$target'" >&2
else
  if ! tmux send-keys -t "$target" '/clear' Enter 2>/dev/null; then
    echo "watcherd: tmux send-keys '/clear' to '$target' failed · marker dropped" >&2
    rm -f "$MARKER_FILE"
    exit 1
  fi
  sleep 1   # let Claude Code process /clear before sending the next command
  if ! tmux send-keys -t "$target" '/handoff:handoff resume' Enter 2>/dev/null; then
    echo "watcherd: tmux send-keys '/handoff:handoff resume' to '$target' failed (note: /clear already sent) · marker dropped" >&2
    rm -f "$MARKER_FILE"
    exit 1
  fi
  echo "watcherd: rotation $rid · sent /clear + /handoff:handoff resume to '$target'" >&2
fi

# 5. Consume marker on the GREEN path.
rm -f "$MARKER_FILE"
exit 0
