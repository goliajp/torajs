# Changelog

All notable changes to `torajs-pool` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-22

### Added

- Initial crate scaffold extracted from torajs runtime's
  P-PERF.A6 "Promise free-list pool" (commit `8f754ca`,
  2026-05-22). That commit delivered -41 % wall-clock on
  promise-await-100k, -36 % on async-fn-call-100k, and
  -32 % on promise-then-100k for the integrated torajs
  AOT binary by replacing per-`Promise` `malloc` / `free`
  with a bounded LIFO free-list.
- `FixedPool<T, const CAP: usize>` — const-generic capacity,
  single-threaded LIFO. Caller supplies the byte-offset of a
  pointer-sized "next" field within `T`; pool reuses that
  field as the free-list link while parked, leaving no
  per-entry bookkeeping overhead.
- `acquire(&self) -> *mut T` — hot pop OR cold fresh-alloc.
- `release(&self, p)` — hot push OR overflow `dealloc`
  when pool is at `CAP`.
- `pooled() -> usize` / `capacity() -> usize` — debug +
  telemetry helpers.
- `Drop` walks the free-list and `dealloc`s every parked
  entry.
- 5 unit tests covering empty / LIFO / bound / drop /
  layout invariants.
- 3 criterion micro-benches: `acquire_release_hot`,
  `acquire_cold_malloc_baseline`, `release_overflow_bound`.
- `tests/perf_gate.rs` regression gate with 100k iter
  budget at 5 ms (≈ 12× headroom over observed P95).

### Notes

- `#![no_std] + extern crate alloc` — no `std` dependency,
  works on `no_std + alloc` targets.
- Single-threaded by construction; `FixedPool: !Sync`.
  Caller wraps in `Mutex` when threading lands.
- API is `unsafe` end-to-end — pool returns / consumes
  raw `*mut T`; caller is responsible for construction /
  destruction of fields other than the "next" link.
