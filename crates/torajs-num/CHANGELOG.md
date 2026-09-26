# Changelog

All notable changes to `torajs-num` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-23

### Added

- Initial release: JavaScript `Number` primitives and the full
  `Math` namespace.
- **`math.rs`**: all 27 `Math` namespace functions
  via Rust `f64` methods. `__torajs_math_round` preserves
  ES §20.2.2.28 "floor(x + 0.5)" semantics (NOT libc tie-to-
  even). `__torajs_math_max` / `_min` propagate `NaN`.
- **`parse.rs`**: `parseInt(s, radix)` +
  `parseFloat(s)` per ES §19.2.5 / §19.2.4. ASCII-only
  whitespace + sign + `0x` prefix + trailing-junk tolerance.
- **`predicates.rs`**: `isNaN` / `isFinite` /
  `Number.isInteger` / `Number.isSafeInteger` — all four with
  separate f64 + i64 variants (the i64 form is constant-true
  for `isInteger` / `isFinite` since the ABI tag already
  selected integer; the safe-integer i64 form does the
  `±(2^53 - 1)` range check).
- **`tostring.rs` + `format.rs` + `to_str.rs`**:
  `Number.prototype.toString(radix)`, `toFixed`,
  `toExponential`, `toPrecision`. Half-away-from-zero rounding
  for `toFixed`; half-even for `toExponential` /
  `toPrecision`; trailing-zero strip per spec.
- **`object_is.rs` + `print_err.rs` + `str_bridge.rs`**: small
  utilities — `Object.is` numeric path, error-printer for
  non-number toString, and Str-bridging for parse-error
  reporting.
