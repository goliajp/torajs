# Changelog

All notable changes to `torajs-str` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added — Full port of `runtime_str.c`'s string surface (P3.1-a through P7.b)

The torajs P3.1 phase + P7.{a,b,c,h-arr,h-json} sub-steps moved ~5
KLOC of string ops from `runtime_str.c` to this crate over a long
sequence:

- **P3.1-a** (`32ab57f`, 2026-05-23): scaffold + `layout.rs` /
  `alloc.rs` / `pool.rs`.
- **P3.1-b** (`9c3ef5b`, 2026-05-23): Substr heap type +
  `substr.rs`.
- **P3.1-c** (`8b4f73a`, 2026-05-23): `eq.rs` byte-equality +
  literals static `.rodata` blocks.
- **P3.1-d** (`b51949f`, 2026-05-23): `concat.rs` +
  `to_number.rs` (Number(s) conversion).
- **P3.1-e** (`...`, 2026-05-23): `transform/` family (upper /
  lower / repeat / pad / trim).
- **P3.1-f** (`...`, 2026-05-23): `split/` family + SplitIter.
- **P3.1-g** (`...`, 2026-05-23): print + IR-side ports +
  closer (the last `_str_*` extern in `runtime_str.c` was deleted).
- **P7.a** (`f30cd8e`, 2026-05-24): Substr method helpers
  (`substr_methods.rs` view-aware dispatch).
- **P7.b** (`ec5af0b`, 2026-05-24): Symbol family (`symbol.rs`).
- **P7.c** (`954a24b`, 2026-05-24): json_quote_str + print_*_err
  helpers.
- **P7.h-arr** (`7373eb4`, 2026-05-24): Array transform helpers
  (moved to torajs-arr later).
- **P7.h-json** (`dab695e`, 2026-05-24): `json_parse/`
  (3-file structured directory: escape / unicode / parser).
- **P7.i-closer** (`21a0feb`, 2026-05-24): last C-side fn
  (`__torajs_object_is_f64`) moved to torajs-num + runtime_str.c
  reduced to a 40-line comment-only stub.

### Polished (2026-05-25)

- LICENSE-MIT + LICENSE-APACHE dual-license.
- README.md expanded from the 47-line P3.1-a scaffold to a full
  publishable shape: badges + module table (18 source files +
  4 sub-directories) + ABI-invariants table + spec-coverage list +
  layout diagram.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0; the full
  P3.1 / P7 ship history per sub-step.
- BUDGETS.md (per-op latency budgets for the hot paths —
  pool acquire / release, charCodeAt, slice, concat, eq).
- `tests/string_ops.rs` — black-box tests for the public extern
  surface covering edge cases (empty string slice, negative
  indices in `slice`, `startsWith` at exact length, `indexOf` with
  haystack < needle, `concat` with one-empty operand).
- `tests/substr_view.rs` — Substr-typed dispatch round-trips
  through `slice` / `charCodeAt` / `eq`.
- `tests/literals.rs` — static `.rodata` Str sharing + the
  FLAG_STATIC_LITERAL refcount no-op contract.
- `benches/str.rs` — criterion benches on the workspace's hot
  shape (small-Str pool churn 100k, charCodeAt of 64-byte input
  100k, slice 64-byte 100k).
- Cargo.toml: criterion dev-dep + [[bench]] section.
