# Changelog

All notable changes to `torajs-num` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-23

### Added

- Initial crate scaffold extracted from torajs P3.2 (closed
  2026-05-23). Replaces ~1.4 KLOC of Number / Math intrinsics
  in the former `runtime_str.c`.
- **`math.rs`** (~400 LOC): all 27 `Math` namespace functions
  via Rust `f64` methods. `__torajs_math_round` preserves
  ES §20.2.2.28 "floor(x + 0.5)" semantics (NOT libc tie-to-
  even). `__torajs_math_max` / `_min` propagate `NaN`.
- **`parse.rs`** (~290 LOC): `parseInt(s, radix)` +
  `parseFloat(s)` per ES §19.2.5 / §19.2.4. ASCII-only
  whitespace + sign + `0x` prefix + trailing-junk tolerance.
- **`predicates.rs`** (~170 LOC): `isNaN` / `isFinite` /
  `Number.isInteger` / `Number.isSafeInteger` — all four with
  separate f64 + i64 variants (the i64 form is constant-true
  for `isInteger` / `isFinite` since the ABI tag already
  selected integer; the safe-integer i64 form does the
  `±(2^53 - 1)` range check).
- **`tostring.rs` + `format.rs` + `to_str.rs`** (~525 LOC):
  `Number.prototype.toString(radix)`, `toFixed`,
  `toExponential`, `toPrecision`. Half-away-from-zero rounding
  for `toFixed`; half-even for `toExponential` /
  `toPrecision`; trailing-zero strip per spec.
- **`object_is.rs` + `print_err.rs` + `str_bridge.rs`**: small
  utilities — `Object.is` numeric path, error-printer for
  non-number toString, and Str-bridging for parse-error
  reporting.

### Polished (2026-05-25)

- README.md with badges + Quick start + spec-compliance notes +
  out-of-scope delimiters.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `tests/spec_compat.rs` — black-box tests for the spec-
  compliance edge cases: `Math.round` half-away-from-zero,
  `Math.max` / `_min` NaN propagation, `parseInt` `0x`-prefix
  + radix interaction, `parseFloat` Infinity acceptance,
  `Number.isSafeInteger` boundary at `2^53 - 1`.
- `benches/num.rs` — criterion benches on the hot-loop usage
  (`Math.sqrt` / `Math.pow` / `parseInt`) at workloads matching
  the torajs bench corpus (popcount-style number-crunch loops).
- `BUDGETS.md` — per-call latency budgets for the bench corpus
  hot paths; documents that libm wrap cost is the floor and
  Rust adds zero overhead on top.
