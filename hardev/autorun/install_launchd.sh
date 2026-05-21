#!/usr/bin/env bash
#
# hardev autorun pillar — install LaunchAgent.
#
# Substitutes the plist template, drops it into ~/Library/LaunchAgents,
# and loads it under the GUI launchd domain. Idempotent — re-runs replace
# any existing load.
#
# Usage:
#   hardev/autorun/install_launchd.sh                  # --dry-run (default)
#   hardev/autorun/install_launchd.sh --mode --apply   # opt into real send-keys
#   hardev/autorun/install_launchd.sh --mode --dry-run # explicit dry-run
#
# --mode <flag> sets the second ProgramArguments string passed to
# watcherd. Valid values: --dry-run (default; safe), --apply (real
# tmux send-keys; enable after P1.5 dogfood verified GREEN).

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
. "$SCRIPT_DIR/lib.sh"

MODE='--dry-run'
while [ $# -gt 0 ]; do
  case "$1" in
    --mode)
      shift
      MODE="${1:-}"
      [ -z "$MODE" ] && { echo "install_launchd: --mode needs a value" >&2; exit 2; }
      case "$MODE" in
        --dry-run|--apply) ;;
        *) echo "install_launchd: invalid --mode '$MODE' (expected --dry-run or --apply)" >&2; exit 2 ;;
      esac
      shift
      ;;
    *)
      echo "install_launchd: unknown arg '$1'" >&2
      exit 2
      ;;
  esac
done

PLIST_SRC="$SCRIPT_DIR/com.hardev.autorun.plist.template"
PLIST_DST="$HOME/Library/LaunchAgents/com.hardev.autorun.plist"
LOG_DIR="$HOME/Library/Logs/hardev"

if [ ! -f "$PLIST_SRC" ]; then
  echo "install_launchd: template missing at $PLIST_SRC" >&2
  exit 1
fi

mkdir -p "$LOG_DIR" "$(dirname "$PLIST_DST")"

# Substitute placeholders.
sed -e "s|@@PROJECT_DIR@@|$PROJECT_DIR|g" \
    -e "s|@@HOME@@|$HOME|g" \
    -e "s|@@MODE@@|$MODE|g" \
    "$PLIST_SRC" > "$PLIST_DST"

# Validate plist syntax.
if ! plutil -lint "$PLIST_DST" >/dev/null; then
  echo "install_launchd: generated plist failed plutil -lint" >&2
  echo "  template: $PLIST_SRC" >&2
  echo "  output:   $PLIST_DST" >&2
  exit 1
fi

# Unload if already loaded (bootout is no-op if absent).
launchctl bootout "gui/$UID" "$PLIST_DST" 2>/dev/null || true

# Load.
if launchctl bootstrap "gui/$UID" "$PLIST_DST"; then
  echo "install_launchd: com.hardev.autorun loaded"
  echo "  plist:    $PLIST_DST"
  echo "  log out:  $LOG_DIR/autorun.out.log"
  echo "  log err:  $LOG_DIR/autorun.err.log"
  echo
  echo "Status:"
  launchctl print "gui/$UID/com.hardev.autorun" 2>/dev/null | grep -E '\b(state|program)\b' | sed 's/^/  /' || echo "  (launchctl print returned nothing — recently-loaded entries may take a moment to appear)"
  echo
  echo "Mode: $MODE"
  if [ "$MODE" = '--dry-run' ]; then
    echo "      (watcherd logs would-be send-keys but does not act)"
    echo "      To go live, re-run: install_launchd.sh --mode --apply"
  else
    echo "      (watcherd will actually tmux send-keys — make sure HARDEV_AUTORUN_TMUX_TARGET"
    echo "       env var is set, or a 'hardev-autorun-sink' tmux session exists, or a pane"
    echo "       whose command matches 'claude'/'node' is running)"
    echo "      To go back to dry-run: install_launchd.sh --mode --dry-run"
  fi
else
  echo "install_launchd: launchctl bootstrap failed" >&2
  exit 1
fi
