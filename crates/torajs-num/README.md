# torajs-num

[![Crates.io](https://img.shields.io/crates/v/torajs-num?style=flat-square&logo=rust)](https://crates.io/crates/torajs-num)
[![docs.rs](https://img.shields.io/docsrs/torajs-num?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-num)
[![License](https://img.shields.io/crates/l/torajs-num?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-num?style=flat-square)](https://crates.io/crates/torajs-num)

JavaScript / ECMAScript `Number` primitives + the full `Math` namespace
for an AOT-compiled TypeScript runtime. 0 Cargo deps. Wraps Rust
`f64` methods (which themselves delegate to libm) and adds spec-mandated
semantics that don't match libm directly (e.g. `Math.round` uses
ES §20.2.2.28 "floor(x + 0.5)" rather than libc `round`'s tie-to-even).

Extracted from the [torajs] AOT TypeScript runtime as **P3.2** (closed
2026-05-23). Replaces ~1.4 KLOC of Number / Math intrinsics in the
former `runtime_str.c` with a structured Rust port covering:

- **Math namespace** (`math.rs`, ~400 LOC): all 27 functions —
  `sqrt`, `abs`, `floor`, `ceil`, `round`, `trunc`, `cbrt`, `exp`,
  `expm1`, `log`, `log1p`, `log2`, `log10`, `sin`/`cos`/`tan`/...,
  `pow`, `min`/`max`, `random`/`imul`/`clz32`/`fround`, `sign`,
  `hypot`, `atan2`.
- **Predicates** (`predicates.rs`, ~170 LOC): `isNaN`, `isFinite`,
  `Number.isInteger`, `Number.isSafeInteger` (both f64 + i64 variants).
- **Parse** (`parse.rs`, ~290 LOC): `parseInt(s, radix)` +
  `parseFloat(s)` per ES §19.2.5 / §19.2.4 (the actual byte-level
  scan + non-letter / signed-leading / NaN-on-empty edge cases).
- **toString / format** (`tostring.rs` + `format.rs` + `to_str.rs`,
  ~525 LOC): `Number.prototype.toString(radix)` + `toFixed` +
  `toExponential` + `toPrecision` with spec-correct rounding modes.
- **Object.is** + heap-Number predicates (`object_is.rs` +
  `print_err.rs` + `str_bridge.rs`).

## Quick start

Pure-Rust usage (workspace internal):

```rust
use torajs_num::parse::{parse_int, parse_float};

assert_eq!(parse_int(b"42", 10), 42.0);
assert_eq!(parse_int(b"0xff", 16), 255.0);
assert_eq!(parse_int(b"  -7  ", 10), -7.0);
assert!(parse_float(b"not-a-number").is_nan());
```

The `extern "C"` `__torajs_*` symbols are the cross-tier ABI for the
torajs AOT code-emit:

```c
// Math intrinsics
double __torajs_math_sqrt(double);
double __torajs_math_pow(double, double);
double __torajs_math_atan2(double, double);
// ... 27 in total

// Predicates
int64_t __torajs_num_is_nan_f(double);
int64_t __torajs_num_is_safe_integer_i(int64_t);
// ...

// Parsing
double __torajs_num_parse_int(const uint8_t *s, int64_t radix);
double __torajs_num_parse_float(const uint8_t *s);
```

## Spec-compliance notes

- **Math.round** — `floor(x + 0.5)`, NOT libm's tie-to-even.
  ES §20.2.2.28.
- **Math.max / min** — return `NaN` if any arg is `NaN`. ES §20.2.2.24 /
  §20.2.2.25.
- **Number.isSafeInteger** — `[-(2^53 - 1), 2^53 - 1]` range check
  with `Number.isInteger` precondition. ES §20.1.2.5.
- **parseInt** — strips ASCII whitespace, parses sign, accepts
  `0x` / `0X` prefix when radix is 16 or 0, ignores trailing junk.
  Returns `NaN` on empty / non-numeric. ES §19.2.5.
- **parseFloat** — accepts a single decimal-point form, exponent
  `[eE][+-]?d+`, `Infinity`, `-Infinity`. Returns `NaN` on empty /
  non-numeric. ES §19.2.4.

## What's NOT in scope (or planned)

- **BigInt operations**: separate crate (`torajs-bigint`).
- **Intl.NumberFormat**: future work, hasn't been scoped yet.
- **Internationalized parsing**: only ASCII whitespace / digits. Per
  ES spec, `parseInt` / `parseFloat` always use ASCII regardless of
  locale.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
