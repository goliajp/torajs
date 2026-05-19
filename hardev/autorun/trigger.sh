#!/usr/bin/env bash
#
# hardev autorun pillar — manual rotation trigger.
#
# Usage:
#   hardev/autorun/trigger.sh              # default: manual
#   hardev/autorun/trigger.sh manual       # takagi-initiated
#   hardev/autorun/trigger.sh self         # agent-self-initiated
#                                          # (CLAUDE.md HARD RULE step 2)
#
# Effect:
#   1. Generates a unique rotation_id.
#   2. Writes .claude/autorun-intent containing the rotation_id (the
#      future Stop hook will read this; P0 just leaves it as breadcrumb).
#   3. Appends a schema-stable JSON line to hardev/autorun/rotations.jsonl.
#   4. Prints next-step instructions to stdout.
#
# Does NOT clear context, does NOT run /handoff. P0 is observation +
# protocol — automation is P1 once we have a measured baseline.

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/lib.sh"

TRIGGER="${1:-manual}"
case "$TRIGGER" in
  self|manual|hook|daemon) ;;
  *)
    echo "trigger.sh: unknown trigger source '$TRIGGER'" >&2
    echo "  expected: self | manual | hook | daemon" >&2
    exit 2
    ;;
esac

# .claude/ must exist (handoff lives there too); fail loud if not.
if [ ! -d "$CLAUDE_DIR" ]; then
  echo "trigger.sh: $CLAUDE_DIR not found — is this a torajs / hardev-managed project?" >&2
  exit 2
fi

rotation_id=$(autorun_new_id)

# 1. intent file (used by P1 Stop hook; also a discoverable trace in P0).
printf '%s\n' "$rotation_id" > "$INTENT_FILE"

# 2. JSON log line.
autorun_record_rotation "$rotation_id" "$TRIGGER"

# 3. Operator instructions. Compact, machine-greppable header line first.
project=$(autorun_project_name)
head=$(autorun_git_head)
echo "hardev autorun: rotation $rotation_id triggered ($TRIGGER) · $project @ $head"
echo
echo "  intent: $INTENT_FILE"
echo "  log:    $ROTATIONS_LOG"
echo
echo "next steps (P0 is manual; P1 will automate):"
echo "  1. agent runs /handoff:handoff save"
echo "  2. user runs /clear"
echo "  3. user runs /handoff:handoff resume"
echo
echo "the new session inherits state from .claude/handoff.md."
