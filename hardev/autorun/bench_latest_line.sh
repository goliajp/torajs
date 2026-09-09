#!/usr/bin/env bash
# Render the committed bench stamp as one line, without touching it.
# bench_latest.sh WRITES the stamp (it reduces a results file); this only
# READS it, because a gate that rewrites its own evidence while judging is
# not a gate.
set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
JSON="${HARDEV_BENCH_JSON:-$PROJECT_DIR/hardev/bench-latest.json}"
[ -f "$JSON" ] || { echo "bench_latest_line: no stamp at $JSON" >&2; exit 2; }
python3 -c '
import json, sys
d = json.load(open(sys.argv[1]))
print("bench: head={headSha} median={medianVsBunAot} target={target} "
      "shortfall={shortfallX}x cells={cells} slower={cellsSlowerThanBunAot} "
      "runs={runs} lowConfidence={lc} ran={benchRanAt}".format(
          lc=str(d.get("lowConfidence")).lower(), **d))
' "$JSON"
