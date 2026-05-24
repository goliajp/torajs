# Changelog — torajs-promise

Per [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] - 2026-05-24

Initial ship as P6.1 (commit `b6f2a71`, 2026-05-24) — full Promise<T>
surface (1071 C LOC → 1.3 KLOC Rust). 7 modules + bounded free-list
pool + thenable absorption + 4 sync combinators + queueMicrotask.

### Polished (2026-05-25)

LICENSE-MIT + LICENSE-APACHE; README with state machine diagram +
combinator table + pool perf delta numbers + module table;
BUDGETS.md; benches/promise.rs placeholder.
