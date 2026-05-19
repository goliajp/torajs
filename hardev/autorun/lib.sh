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
ROTATIONS_LOG="$AUTORUN_DIR/rotations.jsonl"
INTENT_FILE="$CLAUDE_DIR/autorun-intent"
MARKER_FILE="$CLAUDE_DIR/autorun-marker"
HANDOFF_FILE="$CLAUDE_DIR/handoff.md"

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

# Grep the project's most-recent status memory header for an `NNN/0/N`
# conformance triple. Best-effort — returns empty string when memory
# isn't readable (no fabrication).
autorun_conformance_now() {
  local base mem
  base="$HOME/.claude-profile-2/projects/-Users-doracawl-workspace-goliajp-torajs/memory"
  if [ ! -d "$base" ]; then
    base="$HOME/.claude-profile-1/projects/-Users-doracawl-workspace-goliajp-torajs/memory"
  fi
  if [ ! -d "$base" ]; then
    echo ""
    return
  fi
  mem=$(ls -1 "$base"/project_status_*.md 2>/dev/null | sort | tail -1)
  [ -n "$mem" ] && [ -f "$mem" ] || { echo ""; return; }
  grep -oE '\b[0-9]{2,4}/0/[0-9]+\b' "$mem" | head -1
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

# Append a rotation line to rotations.jsonl. Creates the file on first
# write. trigger= self | manual | hook | daemon (future).
autorun_record_rotation() {
  local rotation_id=$1
  local trigger=$2
  local at ts head handoff_sha handoff_age conf

  at=$(autorun_now_iso)
  ts=$(date +%s)
  head=$(autorun_git_head)
  handoff_sha=$(autorun_handoff_sha)
  handoff_age=$(autorun_file_age_sec "$HANDOFF_FILE")
  conf=$(autorun_conformance_now)

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
    "commitsInSession=null")

  mkdir -p "$(dirname "$ROTATIONS_LOG")"
  printf '%s\n' "$line" >> "$ROTATIONS_LOG"
}
