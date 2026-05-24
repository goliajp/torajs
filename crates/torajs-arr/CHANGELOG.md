# Changelog — torajs-arr

Per [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] - 2026-05-23

Initial ship via P4.1 sub-step sequence (commits `490ea58` through
`b1fa088`, 2026-05-23): scaffold + arr_drop / arr_alloc / push_unchecked
/ reserve / push / shift / arrprops side-table / iter family / slice
/ transform / sort / concat / join / find / index_of / any / from_string
/ print. 16 modules, ~2.7 KLOC.

Subsequent IR-restore polish (B1b / B4-shift / B4-push-unchecked
this session, commits `8f3f39f` / `838cc5b` / `fede277`):
restored inkwell-emitted alwaysinline define_arr_push (187 LOC) +
define_arr_shift (4-memory-op) + define_arr_push_unchecked (5-instr)
from git history to recover the hot-loop perf cross-TU `bl + ret`
overhead. array-sum-1m 21.5 → 12.7 ms (-41%); fifo-queue-100k
1.67 → 1.49 ms; array-map-1m 26.5 → 22.6 ms.

### Polished (2026-05-25)

LICENSE-MIT + LICENSE-APACHE; README with heap layout + module
table + performance highlights cross-referenced against bench
corpus baseline; BUDGETS.md per-op latency; benches/arr.rs placeholder.
