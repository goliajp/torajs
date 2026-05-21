#!/usr/bin/env bash
#
# hardev autorun pillar — uninstall LaunchAgent.
#
# Unloads com.hardev.autorun from the GUI launchd domain and removes the
# plist. Leaves logs under ~/Library/Logs/hardev/ intact for audit.
#
# Usage:
#   hardev/autorun/uninstall_launchd.sh

set -u

PLIST_DST="$HOME/Library/LaunchAgents/com.hardev.autorun.plist"

launchctl bootout "gui/$UID" "$PLIST_DST" 2>/dev/null || true
rm -f "$PLIST_DST"

echo "uninstall_launchd: com.hardev.autorun unloaded · removed $PLIST_DST"
echo "(logs under ~/Library/Logs/hardev/ left intact for audit; rm them by hand if you want)"
