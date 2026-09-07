# hardev autorun pillar — common bash helpers.
#
# Source me, don't run me:
#   . "$(dirname "$0")/lib.sh"
#
# Conventions:
#   - All paths absolute; never trust CWD (autorun may be invoked from
#     anywhere — Stop hook, agent turn, takagi shell).
#   - Zero external deps beyond macOS-built-in (python3 / shasum / git /
#     date / awk). No jq, no perl one-liners.
#   - Functions return strings via stdout; errors via stderr; exit codes
#     reserved for callers that explicitly opt in.

# shellcheck shell=bash

set -u

# ── Path discovery ──────────────────────────────────────────────────────
# AUTORUN_DIR = .../hardev/autorun/ (this file's parent)
AUTORUN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HARDEV_DIR="$(cd "$AUTORUN_DIR/.." && pwd)"
PROJECT_DIR="$(cd "$HARDEV_DIR/.." && pwd)"
CLAUDE_DIR="$PROJECT_DIR/.claude"
# Both the log and the handoff are overridable so the gates can be
# exercised against fixtures (see trig_gate_self_test.sh) without
# touching production state. Unset in every real invocation.
ROTATIONS_LOG="${HARDEV_ROTATIONS_LOG:-$AUTORUN_DIR/rotations.jsonl}"
INTENT_FILE="$CLAUDE_DIR/autorun-intent"
MARKER_FILE="$CLAUDE_DIR/autorun-marker"
HANDOFF_FILE="${HARDEV_HANDOFF_FILE:-$CLAUDE_DIR/handoff.md}"

# ── Identity ────────────────────────────────────────────────────────────
# rotation_id = r-<unix-ts>-<4 hex from $RANDOM>
# Unique enough: a session triggering more than once per second per
# project is operator error; the random suffix collapses the same-second
# case to 1/65536.
autorun_new_id() {
  local ts rnd
  ts=$(date +%s)
  rnd=$(printf '%04x' $(( RANDOM & 0xffff )))
  printf 'r-%s-%s\n' "$ts" "$rnd"
}

# RFC-3339 UTC timestamp, no deps.
autorun_now_iso() {
  date -u +'%Y-%m-%dT%H:%M:%SZ'
}

# ── Project state probes ────────────────────────────────────────────────
autorun_project_name() {
  basename "$PROJECT_DIR"
}

autorun_git_head() {
  git -C "$PROJECT_DIR" rev-parse --short HEAD 2>/dev/null || echo unknown
}

# Returns mtime-age in seconds, or empty string if file missing.
autorun_file_age_sec() {
  local f=$1
  if [ ! -f "$f" ]; then
    echo ""
    return
  fi
  local mtime now
  mtime=$(stat -f %m "$f" 2>/dev/null) || { echo ""; return; }
  now=$(date +%s)
  echo $(( now - mtime ))
}

autorun_handoff_sha() {
  if [ ! -f "$HANDOFF_FILE" ]; then
    echo ""
    return
  fi
  local sum
  sum=$(shasum -a 256 "$HANDOFF_FILE" 2>/dev/null | awk '{print $1}')
  [ -n "$sum" ] && printf 'sha256:%s\n' "$sum" || echo ""
}

# Read the conformance triple `N/F/S` from the handoff's rotate-trigger
# section `gate:` line (part of the TRIG-3 contract).  Empty when the
# line is absent — never fabricated.
#
# 2026-09-07: this used to probe `~/.claude-profile-{2,1}/.../memory/
# project_status_*.md`, a layout two profile generations old.  That
# directory has not existed for months, so the function returned "" on
# every call and `conformanceBefore` was null in every row since.  A
# probe aimed at a world that no longer exists fails in exactly the
# shape of "this rotation had no data" — the failure mode
# `perf-decomposition` §1 names, where a broken instrument reads like an
# absent measurement.  The source is now a line the agent writes by hand
# in the very section TRIG-3 already validates: a measured value with an
# owner, instead of a guess about someone else's file format.
autorun_conformance_now() {
  autorun_handoff_trigger_section \
    | grep -oE '^[[:space:]]*gate:[[:space:]]*[0-9]+/[0-9]+/[0-9]+' \
    | head -1 \
    | grep -oE '[0-9]+/[0-9]+/[0-9]+'
}

# ── JSON line emit ──────────────────────────────────────────────────────
# Build a JSON object literal via Python (macOS ships python3); this
# guarantees correct escaping for the handoff sha / project name / etc.
# Args are key=value pairs; values are emitted as strings unless they
# match an explicit `int:` / `null:` / `raw:` prefix.
autorun_emit_jsonl() {
  python3 - "$@" <<'PY'
import json, sys
out = {}
for arg in sys.argv[1:]:
    if "=" not in arg:
        continue
    k, v = arg.split("=", 1)
    if v == "" or v == "null":
        out[k] = None
    elif v.startswith("int:"):
        try:
            out[k] = int(v[4:])
        except ValueError:
            out[k] = None
    elif v.startswith("raw:"):
        # Already valid JSON literal (e.g. nested object); paste through.
        try:
            out[k] = json.loads(v[4:])
        except ValueError:
            out[k] = v[4:]
    else:
        out[k] = v
print(json.dumps(out, ensure_ascii=False, separators=(",", ":")))
PY
}

# Last `trigger=self` row for this project from rotations.jsonl, as
# `ts<TAB>prevHead`.  Empty when no such row exists (cold start).
# Single source of truth: trig_gate.sh sources this instead of carrying
# its own copy — two spellings of one query is how they drift apart.
autorun_last_self_row() {
  if [ ! -f "$ROTATIONS_LOG" ]; then
    echo ""
    return
  fi
  python3 - "$ROTATIONS_LOG" "$(autorun_project_name)" <<'PY'
import json, sys
log, project = sys.argv[1], sys.argv[2]
last = None
try:
    with open(log) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            if r.get("project") != project or r.get("trigger") != "self":
                continue
            last = r
except FileNotFoundError:
    pass
if last and last.get("ts") is not None and last.get("prevHead"):
    print(str(last["ts"]) + "\t" + last["prevHead"])
PY
}

# Commit count for the current session: `<last self prevHead>..HEAD`.
# Empty on cold start or when git cannot answer — callers decide what an
# unknown means (TRIG-1 SKIPs it; the jsonl records null).
autorun_commits_since_last_self() {
  local row last_head
  row=$(autorun_last_self_row)
  [ -n "$row" ] || { echo ""; return; }
  last_head=$(printf '%s\n' "$row" | cut -f2)
  [ -n "$last_head" ] || { echo ""; return; }
  git -C "$PROJECT_DIR" rev-list --count "$last_head..HEAD" 2>/dev/null || echo ""
}

# The `axis:` line from the handoff rotate-trigger section, normalised to
# uppercase comma-separated letters (`A`, or `A,E`).  Empty if absent.
# TRIG-5 reads it back out of rotations.jsonl to see axis skew over time.
autorun_trigger_axis() {
  autorun_handoff_trigger_section \
    | grep -iE '^[[:space:]]*axis:[[:space:]]*[A-Ea-e]([[:space:]]*,[[:space:]]*[A-Ea-e])*[[:space:]]*$' \
    | head -1 \
    | sed -E 's/^[[:space:]]*[Aa][Xx][Ii][Ss]:[[:space:]]*//; s/[[:space:]]//g' \
    | tr '[:lower:]' '[:upper:]'
}

# Extract the body of the handoff.md "rotate 触发" markdown section.
# Lines between `## ...rotate 触发` and the next `## ` heading. Empty if
# section absent or handoff missing. Used by TRIG-3 / TRIG-4 (trig_gate.sh)
# and by autorun_record_rotation to populate the jsonl `triggerReason` field.
autorun_handoff_trigger_section() {
  if [ ! -f "$HANDOFF_FILE" ]; then
    return
  fi
  awk '
    /^## .*rotate 触发/ { in_section = 1; next }
    in_section && /^## / { in_section = 0 }
    in_section { print }
  ' "$HANDOFF_FILE"
}

# Extract the closed-enum letter from the `reason: <a|b|c|d>` line inside
# the handoff trigger section. Outputs single lowercase letter or empty.
autorun_trigger_reason() {
  autorun_handoff_trigger_section \
    | grep -iE '^[[:space:]]*reason:[[:space:]]*[abcd][[:space:]]*$' \
    | head -1 \
    | awk '{ gsub(/[[:space:]]/, ""); sub(/^[Rr][Ee][Aa][Ss][Oo][Nn]:/, ""); print tolower($0) }'
}

# Append a rotation line to rotations.jsonl. Creates the file on first
# write. trigger= self | manual | hook | daemon (future). triggerReason
# is best-effort from handoff; null when absent (manual triggers may
# legitimately have no handoff reason).
autorun_record_rotation() {
  local rotation_id=$1
  local trigger=$2
  local at ts head handoff_sha handoff_age conf reason axis commits

  at=$(autorun_now_iso)
  ts=$(date +%s)
  head=$(autorun_git_head)
  handoff_sha=$(autorun_handoff_sha)
  handoff_age=$(autorun_file_age_sec "$HANDOFF_FILE")
  conf=$(autorun_conformance_now)
  reason=$(autorun_trigger_reason)
  axis=$(autorun_trigger_axis)
  # 2026-09-07: `commitsInSession` was the literal string "null" here —
  # hardcoded, never once populated across 909 rows, while trig_gate.sh
  # computed exactly this number on every single call and discarded it.
  # The historical distribution had to be reconstructed from `prevHead`
  # after the fact to tune TRIG-1.
  commits=$(autorun_commits_since_last_self)

  local line
  line=$(autorun_emit_jsonl \
    "rotationId=$rotation_id" \
    "at=$at" \
    "ts=int:$ts" \
    "project=$(autorun_project_name)" \
    "trigger=$trigger" \
    "prevHead=$head" \
    "handoffSha=$handoff_sha" \
    "handoffAgeSec=${handoff_age:+int:$handoff_age}" \
    "conformanceBefore=$conf" \
    "commitsInSession=${commits:+int:$commits}" \
    "axis=$axis" \
    "triggerReason=$reason")

  mkdir -p "$(dirname "$ROTATIONS_LOG")"
  printf '%s\n' "$line" >> "$ROTATIONS_LOG"
}
