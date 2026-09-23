#!/usr/bin/env bash
#
# Content-based full sync of the source tree to the build host.
#
# Why this exists, and why it does NOT read `git status`:
#
# `git status` answers "what changed since HEAD". That is a different
# question from "what does the build host have", and the two part company
# every time a file is committed here without being sent over. Rotation
# 680 lost two builds to exactly that: a module was renamed, committed,
# and left stale on mini, and the failure read
# `cannot find function ... in module ...` — which looks like a code error
# and is not one.
#
# `-c` compares by checksum rather than mtime, so only real content
# differences move and cargo rebuilds only the crates that actually
# changed; `--delete` takes away files that no longer exist here, which is
# what keeps a fixture from an abandoned knife out of the next gate's
# case count.
#
# Usage: bash hardev/autorun/sync_to_mini.sh [host] [remote-path]
set -euo pipefail

HOST="${1:-mini}"
REMOTE="${2:-workspace/goliajp/torajs}"
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

[ -f "$REPO/Cargo.toml" ] || { echo "sync_to_mini: $REPO is not the repo root" >&2; exit 2; }
cd "$REPO"

# Refuse to move source while a gate is running on that tree.
#
# A conformance gate exec's `target/<profile>/tr` per fixture, so a
# build started underneath it swaps the binary mid-run and the two
# halves of the result belong to different commits. The rule has been
# written down since rotation 78; rotation 686 broke it twice in one
# session anyway, both times a few seconds after launching a gate in
# the background, when the next knife's pre-flight felt like the next
# thing to do. A rule that is only in a document is a rule that gets
# skipped while the document is true, so it lives here now, where the
# skip is not available.
#
# Override with SYNC_ALLOW_DURING_GATE=1 when the gate is one you have
# already decided to discard.
if [ "${SYNC_ALLOW_DURING_GATE:-0}" != "1" ]; then
  running="$(ssh "$HOST" 'pgrep -f torajs-conformance | wc -l' 2>/dev/null | tr -d " ")"
  if [ -n "$running" ] && [ "$running" != "0" ]; then
    echo "sync_to_mini: a conformance gate is running on $HOST ($running processes)." >&2
    echo "  Building now would swap the binary under it. Wait for it, or" >&2
    echo "  kill it and re-run: ssh $HOST pkill -f torajs-conformance" >&2
    echo "  (SYNC_ALLOW_DURING_GATE=1 to override.)" >&2
    exit 3
  fi
fi

rsync -rlc --no-times --delete --exclude 'target/' crates/ "$HOST:$REMOTE/crates/"
rsync -rlc --no-times --delete conformance/cases/ "$HOST:$REMOTE/conformance/cases/"
# The RUNNER's own source, not just the fixtures it walks. Rotation 696
# changed how `judge_oracle` buckets an async verdict and the change
# could not reach mini — where the sweep actually runs — because only
# `cases/` was ever sent. Same shape as the `hardev/autorun/` gap
# rotation 619 paid for: a directory outside the channel looks exactly
# like a directory whose contents happen to match.
rsync -rlc --no-times --delete --exclude 'target/' conformance/test262-runner/ \
  "$HOST:$REMOTE/conformance/test262-runner/"
rsync -rlc --no-times hardev/autorun/ "$HOST:$REMOTE/hardev/autorun/"
rsync -rlc --no-times --delete --exclude 'target/' bench/ "$HOST:$REMOTE/bench/"

# The judge is the host, not this script's exit code: a dry run that
# prints nothing is the only evidence the trees agree.
DIFFS="$(rsync -rlcn --out-format='%n' --exclude 'target/' crates/ "$HOST:$REMOTE/crates/" | grep -E '\.rs$' || true)"
if [ -n "$DIFFS" ]; then
  echo "sync_to_mini: STILL DIFFERING after sync:" >&2
  echo "$DIFFS" >&2
  exit 1
fi
echo "sync_to_mini: crates/ conformance/{cases,test262-runner}/ hardev/autorun/ bench/ in sync with $HOST"
