# Changelog

All notable changes to `torajs-pool` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-22

### Added

- `FixedPool<T, const CAP: usize>` — const-generic capacity,
  single-threaded LIFO. Caller supplies the byte-offset of a
  pointer-sized "next" field within `T`; pool reuses that
  field as the free-list link while parked, leaving no
  per-entry bookkeeping overhead. Replaces per-item `malloc` /
  `free` with a bounded LIFO free-list.
- `acquire(&self) -> *mut T` — hot pop OR cold fresh-alloc.
- `release(&self, p)` — hot push OR overflow `dealloc`
  when pool is at `CAP`.
- `pooled() -> usize` / `capacity() -> usize` — debug +
  telemetry helpers.
- `Drop` walks the free-list and `dealloc`s every parked
  entry.

### Notes

- `#![no_std] + extern crate alloc` — no `std` dependency,
  works on `no_std + alloc` targets.
- Single-threaded by construction; `FixedPool: !Sync`.
  Caller wraps in `Mutex` when threading lands.
- API is `unsafe` end-to-end — pool returns / consumes
  raw `*mut T`; caller is responsible for construction /
  destruction of fields other than the "next" link.
