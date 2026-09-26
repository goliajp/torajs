# Changelog

All notable changes to `torajs-str` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

Full `Str` + `Substr` string surface:

- Heap layout + pool-aware allocation (`layout.rs` / `alloc.rs` /
  `pool.rs`); `Substr` heap type (`substr.rs`).
- Byte-equality (`eq.rs`) with static `.rodata` literal blocks.
- `concat.rs` + `Number(s)` conversion (`to_number.rs`).
- `transform/` family: `toUpperCase` / `toLowerCase` / `repeat` /
  `padStart` / `padEnd` / `trim`.
- `split/` family + `SplitIter`.
- Substr-typed method dispatch (`substr_methods.rs`).
- `Symbol` family (`symbol.rs`).
- `JSON.stringify` string-body quoting + `json_parse/` (escape
  decode, unicode escape, surrogate pair handling).
