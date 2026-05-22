# torajs-ucd performance budgets

Latency budgets enforced by `tests/perf_gate.rs`. Each budget is set
with ~3× headroom over the observed P95 on a dev machine. Binary
search is intrinsically O(log N) cheap so there's not much
microbench-noise budget to give up — anything beyond ~3× is a real
algorithm regression worth chasing.

Run `cargo test -p torajs-ucd --test perf_gate` to check.

## Path taxonomy

`is_letter_cp` / `is_number_cp` are **hot** path for regex `\p{NAME}`
class matching when the pattern has the `u` flag. Each cp ≥ 128 read
from the input string runs one of these on the regex VM's bytecode
dispatch loop. For a million-codepoint input on a `\p{L}+` pattern
that's one million `is_letter_cp` calls — per `rules/rust/patterns.md`
hot path budget sits at ns / op.

## Budgets

| Path | Budget | Observed P95 (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `is_letter_cp` ×~63k | 50 ms | ~15 ms | ~3× | Binary search log2(~50) ≈ 6 iters → ~15 ns each on aarch64 M-series |
| `is_number_cp` ×~63k | 50 ms | ~12 ms | ~4× | Same shape, smaller table (~30 ranges) → ~10 ns each |

## Methodology

- Each test sweeps a range of codepoints (0x0000..~0x4000) calling
  the lookup once per cp. Mix of hits and misses across the U+0000–
  U+9FFF region.
- The **median** sample is asserted under the budget, not the mean —
  median is robust to occasional GC / context-switch noise.
- Budgets are **wall-clock**, not CPU time.

## When to re-measure

Update the table (and asserts in `perf_gate.rs`) when any of these
fire:

- Table size changes (added a script's letters / digits to the
  range table)
- Algorithm change (e.g. switch to perfect hash or interval tree)
- Code-gen change (rustc / LLVM upgrade)
- A criterion bench median in `benches/ucd.rs` moves by > 30%
  in either direction — that's a real signal worth chasing.

## Future work

- Auto-import from `UnicodeData.txt` to get the full UCD coverage
  rather than the current curated subset. The current `UCD_LETTER`
  / `UCD_NUMBER` cover the dominant test262 cases but miss less-
  common scripts (Lao, Ogham, Tifinagh, etc.). A v1.0 expansion
  could pull from a UCD-version-pinned generator that emits the
  table file at crate build time.
