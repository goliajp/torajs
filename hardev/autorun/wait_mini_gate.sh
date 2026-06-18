#!/usr/bin/env bash
#
# hardev autorun pillar — block until a mini conformance gate run
# finishes (or a timeout / progress-stall trips), then echo the final
# summary line for the caller's transcript.
#
# Why this exists:
#   autorun loops repeatedly need to "wait for the conformance gate
#   on mini to publish its `XXX pass / Y fail / Z skip` line". The
#   naive pattern
#
#       until ssh mini "grep -q 'results:' /tmp/gate-X.log"; do sleep 30; done
#
#   runs as a long-lived background Bash on the dev machine and gets
#   SIGRTMIN+16'd (exit 144) by the Claude Code harness once its
#   ~5min background-process budget elapses. The watcher disappears
#   silently and the autorun loop loses sync. This script avoids the
#   trap by doing the entire wait **inside a single SSH session on
#   mini**, where `tail -F | grep -m1 …` blocks until the pattern is
#   seen — the dev-side bash is foreground, so the harness's longer
#   foreground timeout (10min) applies.
#
# Usage:
#   wait_mini_gate.sh <log-path-on-mini>
#
# Behaviour:
#   - SSHes to `mini` and runs `tail -F <log> | grep -m1 -E '<pat>'`
#     where <pat> matches the conformance harness's final summary
#     line (`NNN pass / N fail / N skip`).
#   - Echoes the matching line on stdout, then exits 0.
#   - Falls back to checking whether the line is **already** present
#     before tailing — `grep -m1` on a freshly-`tail -F` stream only
#     sees lines emitted **after** the tail starts, so without the
#     pre-check a gate that already finished would block forever.
#   - Refuses to wait longer than HARDEV_WAIT_MINI_GATE_TIMEOUT
#     seconds (default 540 = 9min, safely under the harness's
#     foreground budget). On timeout exits 2 with a diagnostic.
#
# Env vars:
#   HARDEV_AUTORUN_MINI_HOST   ssh host alias (default: "mini")
#   HARDEV_WAIT_MINI_GATE_TIMEOUT  seconds (default 540)
#
# Exit codes:
#   0 — gate finished, summary printed
#   1 — usage error
#   2 — timed out before gate finished
#   3 — ssh failure (host unreachable, etc.)

set -u

if [ $# -ne 1 ]; then
  echo "usage: wait_mini_gate.sh <log-path-on-mini>" >&2
  exit 1
fi

LOG_PATH="$1"
HOST="${HARDEV_AUTORUN_MINI_HOST:-mini}"
TIMEOUT="${HARDEV_WAIT_MINI_GATE_TIMEOUT:-540}"
PATTERN='[0-9]+ pass / [0-9]+ fail / [0-9]+ skip'

# Fast path: gate may already be done by the time we get called
# (race between launch + wait_mini_gate dispatch). One short ssh to
# grep the existing log avoids the `tail -F` block-forever trap.
existing=$(ssh "$HOST" "grep -m1 -E '$PATTERN' '$LOG_PATH' 2>/dev/null") || {
  rc=$?
  if [ $rc -ne 1 ]; then
    # grep exit 1 = no match (fine, fall through to tail).
    # Anything else = ssh / shell error.
    echo "wait_mini_gate: ssh check failed (rc=$rc)" >&2
    exit 3
  fi
}
if [ -n "$existing" ]; then
  printf '%s\n' "$existing"
  exit 0
fi

# Slow path: poll the log every POLL_INTERVAL seconds. Earlier
# revisions used `tail -F | grep -m1` inside a remote `timeout`
# wrapper, but mini (Apple Silicon, no brew coreutils) ships
# without GNU `timeout`, and `tail -F | grep -m1` alone leaks the
# SSH session — grep exits the moment it matches, but tail-F only
# notices pipe-close when it tries to write another line, which
# may never happen on an already-quiet log. Polling is bounded
# inside the single SSH session by tracking wall time against
# TIMEOUT; ~5s poll interval makes the overhead ~10 round trips
# of one `grep -m1` over the (small) log file for a ~1min gate.
POLL_INTERVAL=5
remote_cmd="
end=\$(( \$(date +%s) + ${TIMEOUT} ))
while [ \$(date +%s) -lt \$end ]; do
  m=\$(grep -m1 -E '${PATTERN}' '${LOG_PATH}' 2>/dev/null || true)
  if [ -n \"\$m\" ]; then printf '%s\n' \"\$m\"; exit 0; fi
  sleep ${POLL_INTERVAL}
done
exit 124
"
match=$(ssh "$HOST" "$remote_cmd")
rc=$?
case "$rc" in
  0)
    printf '%s\n' "$match"
    exit 0
    ;;
  124)
    echo "wait_mini_gate: timed out after ${TIMEOUT}s waiting for gate summary in $LOG_PATH" >&2
    exit 2
    ;;
  *)
    echo "wait_mini_gate: ssh / remote command failed (rc=$rc)" >&2
    exit 3
    ;;
esac
