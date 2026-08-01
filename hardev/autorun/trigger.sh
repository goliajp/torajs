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

# TRIG-1..4 pre-gate (P2.0 SHIPPED). Only self-triggered rotations are
# gated — manual = takagi override path (cases#rotate-as-procrastination
# decision authority remains with the user). See `README.md` → "TRIG-1..4
# spec" + `metrics.md` §6 baseline @2026-06-15.
if [ "$TRIGGER" = "self" ]; then
  if ! "$SCRIPT_DIR/trig_gate.sh"; then
    echo "trigger.sh: self trigger BLOCKED by TRIG gate (see lines above)" >&2
    echo "  · ship more substrate first, or wait wall time, or fix handoff reason." >&2
    echo "  · manual override (takagi only): hardev/autorun/trigger.sh manual" >&2
    exit 1
  fi
fi

# HARD invariant (takagi 2026-08-02): a rotation may not close while
# ANY child process of this session survives — every watcher/poller/
# sleeper is reaped mechanically here, not by agent discipline. Runs
# on every accepted trigger (self AND manual). rotation-276 incident:
# a sweep watcher polling a pattern that never appears ran 6h into
# the next rotation because the ps-based audit truncated its command
# line; this reaper walks the process tree instead.
"$SCRIPT_DIR/kill_stray_shells.sh" || {
  echo "trigger.sh: kill_stray_shells.sh FAILED — rotation close aborted" >&2
  exit 1
}

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
