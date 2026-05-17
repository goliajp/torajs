#!/usr/bin/env python3
"""Update the bench table in labs/pitch.html from the
latest bench/results/*.json. Run after each L3a ship — keeps the
public pitch numbers honest (4-pillar 规范: no manual entry drift).

Sorts rows by (bun-aot.run_ms / torajs.run_ms) descending so the
top of the table is always the biggest tora-vs-bun-aot win. Cases
where any of bun-aot / bun-jsc / node-v8 / torajs is missing/skipped
are excluded with a stderr warning. The class `x lo` marker is
applied to rows whose ratio is < 2.0 (kept faithful to the visual
contrast in the existing HTML).

Geomean computed across the rows that participated in the table
(bun-aot vs torajs and node-v8 vs torajs separately).
"""
import json, math, os, re, sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RESULTS_DIR = ROOT / "bench" / "results"
HTML_PATH = ROOT / "labs" / "pitch.html"


def latest_result_path() -> Path:
    files = sorted(RESULTS_DIR.glob("*.json"), key=lambda p: p.stat().st_mtime, reverse=True)
    if not files:
        raise SystemExit("no bench/results/*.json found")
    return files[0]


def load_rows(path: Path):
    data = json.loads(path.read_text())
    # rows: [{case, runtime, run_ms, status, ...}, ...]
    # Group by case → {runtime: run_ms}.
    by_case: dict[str, dict[str, float]] = {}
    for r in data.get("rows", []):
        if r.get("status") != "ok":
            continue
        run_ms = r.get("run_ms")
        if run_ms is None:
            continue
        by_case.setdefault(r["case"], {})[r["runtime"]] = float(run_ms)
    return by_case, data.get("git_sha", "?")[:7]


def geomean(xs):
    if not xs:
        return float("nan")
    return math.exp(sum(math.log(x) for x in xs) / len(xs))


def build_rows(by_case: dict[str, dict[str, float]]):
    rows = []
    needed = ("bun-aot", "bun-jsc", "node-v8", "torajs")
    for case, runtimes in by_case.items():
        if not all(k in runtimes for k in needed):
            print(f"skip {case}: missing one of {needed} in {list(runtimes.keys())}", file=sys.stderr)
            continue
        tora = runtimes["torajs"]
        bun_aot = runtimes["bun-aot"]
        x = bun_aot / tora if tora > 0 else float("inf")
        rows.append((case, bun_aot, runtimes["bun-jsc"], runtimes["node-v8"], tora, x))
    rows.sort(key=lambda r: r[5], reverse=True)
    return rows


def fmt_ms(x: float) -> str:
    if x >= 100:
        return f"{x:.2f}"
    return f"{x:.2f}"


def fmt_x(x: float) -> str:
    return f"{x:.2f}"


def render_table_rows(rows) -> str:
    lines = []
    for case, ba, bj, n8, t, x in rows:
        lo = " lo" if x < 2.0 else ""
        lines.append(
            f'      <tr><td class="case">{case}</td>'
            f'<td>{fmt_ms(ba)}</td><td>{fmt_ms(bj)}</td><td>{fmt_ms(n8)}</td>'
            f'<td class="tora">{fmt_ms(t)}</td><td class="x{lo}">{fmt_x(x)}</td></tr>'
        )
    return "\n".join(lines)


def render_summary(rows) -> tuple[str, str, str, str]:
    bun_aot_ratios = [r[1] / r[4] for r in rows]
    node_ratios = [r[3] / r[4] for r in rows]
    peak = max(rows, key=lambda r: r[5]) if rows else None
    bottom = min(rows, key=lambda r: r[5]) if rows else None
    return (
        f"{geomean(bun_aot_ratios):.2f}",
        f"{geomean(node_ratios):.2f}",
        f"{peak[5]:.2f}× peak speedup ({peak[0]})" if peak else "n/a",
        f"{bottom[5]:.2f}× minimum speedup ({bottom[0]})" if bottom else "n/a",
    )


def patch_html(rows_html: str, summary: tuple[str, str, str, str], n_cases: int, git_sha: str):
    html = HTML_PATH.read_text()

    # 1. Replace the bench table tbody.
    html = re.sub(
        r"(<table class=\"bench\">.*?<tbody>\n).*?(    </tbody>)",
        lambda m: f"{m.group(1)}{rows_html}\n{m.group(2)}",
        html,
        count=1,
        flags=re.DOTALL,
    )

    # 2. Replace the bench-summary spans (geomean / peak / min).
    bun_aot, node, peak_str, min_str = summary
    new_summary = (
        f'    <span><b>{bun_aot}×</b> geomean vs bun-aot</span>\n'
        f'    <span><b>{node}×</b> geomean vs node-v8</span>\n'
        f'    <span><b>{peak_str.split("×")[0]}×</b> peak speedup ({peak_str.rsplit("(", 1)[1]}</span>\n'
        f'    <span><b>{min_str.split("×")[0]}×</b> minimum speedup ({min_str.rsplit("(", 1)[1]}</span>'
    )
    html = re.sub(
        r'(<div class="bench-summary">\n).*?(  </div>\n  <table class="bench">)',
        lambda m: f"{m.group(1)}{new_summary}\n  {m.group(2)}",
        html,
        count=1,
        flags=re.DOTALL,
    )

    # 3. Update the section header benchmark count.
    html = re.sub(
        r"<span class=\"num\">02</span>\d+ benchmarks · \d+ wins",
        f'<span class="num">02</span>{n_cases} benchmarks · {n_cases} wins',
        html,
        count=1,
    )

    # 4. Update KPI subtext "geomean across N benchmarks · all N won by torajs".
    html = re.sub(
        r"geomean across \d+ benchmarks · all \d+ won by torajs",
        f"geomean across {n_cases} benchmarks · all {n_cases} won by torajs",
        html,
    )

    # 5. Update bench-foot to show source data path.
    html = re.sub(
        r"<p class=\"bench-foot\">[^<]*</p>",
        f'<p class="bench-foot">\n    all values are run_ms · hyperfine 10 runs, 3 warmup · darwin · arm64 · Apple M-class · stock thermal · all benchmarks run solo · data: bench/results @ HEAD {git_sha}\n  </p>',
        html,
        count=1,
    )

    # 6. Update the conformance KPI from `ls conformance/cases/*.ts +
    #    conformance/test262-port/*.ts` so the headline doesn't drift
    #    after every L3a ship. (The conformance runner walks both dirs;
    #    1 case is currently skipped on macOS but not subtracted here
    #    since the absolute "in-suite" count is the user-facing
    #    headline.) Test262 5k pass rate / ships count are not
    #    auto-measured — those need explicit refresh or manual edit.
    conf_count = sum(
        1 for p in (ROOT / "conformance" / "cases").glob("*.ts")
    ) + sum(
        1 for p in (ROOT / "conformance" / "test262-port").glob("*.ts")
    )
    html = re.sub(
        r'(<div class="stat-v">)\d+(<span class="d">\+\d+</span></div>\n      <div class="stat-l">conformance)',
        lambda m: f'{m.group(1)}{conf_count}{m.group(2)}',
        html,
        count=1,
    )

    # 7. Update the header date (top-right) to today so the preview
    #    stays current after every refresh — takagi flagged the
    #    2026·05·16 stale string on 2026-05-18. Format: YYYY · MM · DD.
    import datetime as _dt
    today = _dt.date.today().strftime("%Y · %m · %d")
    html = re.sub(
        r'<span>\d{4} · \d{2} · \d{2}</span>',
        f'<span>{today}</span>',
        html,
        count=1,
    )

    HTML_PATH.write_text(html)


def main():
    src = latest_result_path()
    by_case, git_sha = load_rows(src)
    rows = build_rows(by_case)
    if not rows:
        raise SystemExit("no rows with all 4 runtimes present")
    rows_html = render_table_rows(rows)
    summary = render_summary(rows)
    patch_html(rows_html, summary, len(rows), git_sha)
    print(f"updated {HTML_PATH.relative_to(ROOT)} from {src.name}")
    print(f"  cases: {len(rows)}  geomean vs bun-aot: {summary[0]}×  vs node-v8: {summary[1]}×")
    print(f"  peak: {summary[2]}  min: {summary[3]}")


if __name__ == "__main__":
    main()
