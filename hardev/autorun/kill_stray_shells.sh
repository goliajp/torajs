#!/bin/bash
# kill_stray_shells.sh — rotation-close hard invariant (takagi
# 2026-08-02): when a rotation ends, EVERY child process this
# Claude Code session spawned must be dead. No watcher, poller,
# sleeper, or remote-wait shell may survive into the next rotation.
#
# Mechanism: walk up from $$ to the owning `claude` process, then
# enumerate its descendants. A Bash-tool shell is identified by the
# `shell-snapshots` marker in its command line (every Bash tool
# invocation wraps as `/bin/zsh -c source .../shell-snapshots/...`);
# such a shell and its whole subtree are victims — EXCEPT the chain
# this very script is running under. Non-shell children of claude
# (MCP servers, IDE helpers) are never touched.
#
# Exit 0 + "CLEAN" when nothing stray; exit 0 + KILL lines after
# reaping. Exit 1 only when no claude ancestor is found (not run
# from within a session — refuse rather than guess).
#
# Also best-effort clears mini-side project processes (conformance /
# test262 / tr spawn-children) — those outlive dev-side shells when
# an ssh link drops.

set -u

me=$$
ancestors=" $me "
claude_pid=""
p=$me
while [ -n "$p" ] && [ "$p" != "0" ] && [ "$p" != "1" ]; do
  cmd=$(ps -p "$p" -o comm= 2>/dev/null)
  case "$cmd" in
    *claude*) claude_pid=$p; break ;;
  esac
  p=$(ps -p "$p" -o ppid= 2>/dev/null | tr -d ' ')
  ancestors="$ancestors$p "
done

if [ -z "$claude_pid" ]; then
  echo "NO_CLAUDE_ANCESTOR — not inside a Claude Code session, refusing"
  exit 1
fi

descendants() {
  local pid=$1 c
  for c in $(pgrep -P "$pid" 2>/dev/null); do
    echo "$c"
    descendants "$c"
  done
}

killed=0
# Bash-tool shells are DIRECT children of claude carrying the
# shell-snapshots marker; kill each such subtree except our own.
for shell in $(pgrep -P "$claude_pid" 2>/dev/null); do
  case "$ancestors" in *" $shell "*) continue ;; esac
  cmdline=$(ps -p "$shell" -ww -o command= 2>/dev/null)
  case "$cmdline" in
    *shell-snapshots*) ;;
    *) continue ;;
  esac
  for victim in $shell $(descendants "$shell"); do
    case "$ancestors" in *" $victim "*) continue ;; esac
    line=$(ps -p "$victim" -ww -o command= 2>/dev/null | cut -c1-140)
    [ -z "$line" ] && continue
    echo "KILL $victim: $line"
    kill "$victim" 2>/dev/null
    killed=$((killed + 1))
  done
done

# mini-side best-effort reap of project processes (never blocks).
ssh -o ConnectTimeout=5 -o BatchMode=yes mini \
  'pkill -f "torajs-conformance|torajs-test262|torajs-run-new" 2>/dev/null; rm -f /tmp/torajs-run-new-* 2>/dev/null; ls /var/folders/*/T/torajs-run-new-* 2>/dev/null | xargs rm -f 2>/dev/null; true' \
  2>/dev/null || true

if [ "$killed" -eq 0 ]; then
  echo "CLEAN: no stray shells under claude pid $claude_pid"
else
  echo "REAPED $killed stray process(es) under claude pid $claude_pid"
fi
exit 0
