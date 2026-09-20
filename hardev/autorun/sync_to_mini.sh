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

rsync -rlc --no-times --delete --exclude 'target/' crates/ "$HOST:$REMOTE/crates/"
rsync -rlc --no-times --delete conformance/cases/ "$HOST:$REMOTE/conformance/cases/"
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
echo "sync_to_mini: crates/ conformance/cases/ hardev/autorun/ bench/ in sync with $HOST"
