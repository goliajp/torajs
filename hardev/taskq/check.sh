#!/usr/bin/env bash
#
# hardev taskq pillar — L1–L4 plan-source invariant checker.
#
# Asserts the mechanically-decidable, zero-false-positive subset of
# the taskq invariants (hardev/taskq/README.md INV-1…7) against the
# live plan source (the status-memory file the project's 4-layer
# planning runs on). Exit-coded so it works as a session-boundary /
# pre-commit gate, exactly like `bench compare` for perf.
#
#   hardev/taskq/check.sh                 # auto-discover plan source
#   hardev/taskq/check.sh <plan.md>       # explicit (testing)
#
# This v1 enforces only what is robustly scriptable on the freeform
# markdown without fragile heuristics — every check below is grounded
# in a CONCRETE drift caught this session (see taskq/README.md
# D1/D3). Deeper INV-2/3/4/6/7 (cross-ref git/tasks) are follow-on.
#
set -u

REPO="/Users/doracawl/workspace/goliajp/torajs"

# --- locate the plan source --------------------------------------------------
PLAN="${1:-}"
if [ -z "$PLAN" ]; then
  # the torajs project_status memory (newest if several)
  base="$HOME/.claude-profile-2/projects/-Users-doracawl-workspace-goliajp-torajs/memory"
  PLAN=$(ls -1 "$base"/project_status_*.md 2>/dev/null | sort | tail -1)
fi
if [ -z "$PLAN" ] || [ ! -f "$PLAN" ]; then
  echo "hardev taskq check: ERROR — plan source not found (arg: '${1:-<auto>}')" >&2
  exit 2
fi

echo "hardev taskq check — plan source: $PLAN"
fail=0
note() { echo "  $1"; }
violate() { echo "  ‼ INV-$1 VIOLATION: $2"; fail=1; }

# header line = first markdown H1 ("# torajs status …"); the file's
# own protocol makes the top block load-bearing.
header=$(grep -m1 '^# ' "$PLAN")

# --- INV-1a: header HEAD must be the current git HEAD ------------------------
# (caught D1 — header said an old sha while HEAD had moved 10 commits on)
head_sha=$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null)
if [ -z "$head_sha" ]; then
  note "INV-1a SKIP (not a git tree?)"
elif printf '%s' "$header" | grep -qF "$head_sha"; then
  note "INV-1a OK — header references current HEAD $head_sha"
else
  violate "1a" "header does not reference current git HEAD ($head_sha) — stale single-source-of-truth"
fi

# --- INV-1b: if focus=hardev, header must name the current hardev version ----
# (D1-class — header version lagging the shipped VERSION)
if grep -qiE 'focus *= *.?hardev|新 L2 focus = .?hardev' "$PLAN"; then
  hv=$(cat "$REPO/hardev/VERSION" 2>/dev/null | tr -d '[:space:]')
  if [ -n "$hv" ] && printf '%s' "$header" | grep -qF "hardev v$hv"; then
    note "INV-1b OK — header names current hardev v$hv"
  else
    violate "1b" "focus=hardev but header does not name current hardev v$hv"
  fi
else
  note "INV-1b N/A (focus is not hardev)"
fi

# --- INV-5: a `## L3a` section that is closed work must be banner-marked -----
# ARCHAEOLOGY, never left inline as if live (caught D3 — the whole
# L3a P7 hot queue was shipped but read as the live plan; the file's
# own "读 L3a 顶部 take 一项" protocol would mis-route a reader).
l3a_head=$(grep -m1 '^## L3a' "$PLAN")
if [ -z "$l3a_head" ]; then
  note "INV-5 N/A (no ## L3a section)"
else
  # Structural marker: the HEADING LINE ITSELF must carry ARCHAEOLOGY
  # when the section is closed work. Greppng the body window is a
  # false-negative trap — any explanatory prose ("closed work →
  # archaeology") matches it even after the marker is stripped. The
  # heading suffix is what de-drift adds and a drifted/reverted state
  # lacks, so it's the unambiguous structural invariant.
  if grep -qiE 'autorun.*(已停|停止|paused)|focus *= *.?hardev' "$PLAN"; then
    if printf '%s' "$l3a_head" | grep -qF 'ARCHAEOLOGY'; then
      note "INV-5 OK — ## L3a HEADING marked ARCHAEOLOGY (live hot is elsewhere per directive)"
    else
      violate "5" "## L3a heading is closed work per directive but the HEADING LINE lacks an ARCHAEOLOGY marker — a reader following 'read L3a top' is mis-routed ($l3a_head)"
    fi
  else
    note "INV-5 OK — directive declares an active L3a (not closed)"
  fi
fi

echo
if [ "$fail" -eq 0 ]; then
  echo "hardev taskq check: PASS — plan source consistent (INV-1a/1b/5)"
  exit 0
else
  echo "hardev taskq check: FAIL — de-drift the plan source (taskq/README.md), do not silence the check"
  exit 1
fi
