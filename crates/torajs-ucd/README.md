# torajs-ucd

[![Crates.io](https://img.shields.io/crates/v/torajs-ucd?style=flat-square&logo=rust)](https://crates.io/crates/torajs-ucd)
[![docs.rs](https://img.shields.io/docsrs/torajs-ucd?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-ucd)
[![License](https://img.shields.io/crates/l/torajs-ucd?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-ucd?style=flat-square)](https://crates.io/crates/torajs-ucd)

Curated Unicode Character Database subset — Letter (`L`) and Number
(`N`) ranges as binary-searchable static tables. Sized for ECMAScript
regex `\p{L}` / `\p{N}` property classes; covers the dominant test262
usage subset. **Zero allocation, `no_std`, zero dependencies.**

Extracted from the [torajs] AOT TypeScript runtime
(`runtime_regex.c::UCD_LETTER` / `UCD_NUMBER`, shipped 2026-05-19 in
P9.3-A2). Provides the same range tables + binary-search lookup as a
standalone Rust crate.

## Quick start

```rust
use torajs_ucd::{is_letter_cp, is_number_cp};

assert!(is_letter_cp(0x4E2D));   // 中 (CJK Unified Ideograph)
assert!(is_letter_cp(0x03B1));   // α (Greek lowercase alpha)
assert!(is_letter_cp(0x05D0));   // א (Hebrew aleph)
assert!(!is_letter_cp(b'a' as u32)); // ASCII — caller bitmap-tests

assert!(is_number_cp(0x0660));   // Arabic-Indic 0
assert!(is_number_cp(0xFF15));   // Fullwidth 5
assert!(!is_number_cp(b'0' as u32)); // ASCII — caller bitmap-tests
```

You can also use the raw range tables directly:

```rust
use torajs_ucd::{UCD_LETTER, UCD_NUMBER, range_contains};

let cp = 0x4E2D;
assert!(range_contains(UCD_LETTER, cp));
```

## API

| Item | Description |
|---|---|
| `Range` | Type alias for `(u32, u32)` — `(lo, hi)` inclusive codepoint range. |
| `UCD_LETTER: &[Range]` | Curated Letter range table (cp ≥ 128). |
| `UCD_NUMBER: &[Range]` | Curated Number range table (cp ≥ 128). |
| `range_contains(table, cp) -> bool` | Generic binary-search lookup. |
| `is_letter_cp(cp) -> bool` | Lookup wrapper over `UCD_LETTER`. |
| `is_number_cp(cp) -> bool` | Lookup wrapper over `UCD_NUMBER`. |

## Scope (what's covered, what's not)

**Covered Letter ranges**: Latin-1 supplement, IPA + Spacing Modifier,
Greek + Coptic, Cyrillic, Armenian, Hebrew, Arabic, Devanagari, Thai,
Hiragana, Katakana, CJK Unified Ideographs (basic + ext A), Hangul
Syllables.

**Covered Number ranges**: Latin-1 numeric, Arabic-Indic, NKo,
Devanagari, Bengali, Gurmukhi, Gujarati, Oriya, Tamil, Telugu,
Kannada, Malayalam, Sinhala, Thai, Lao, Tibetan, Myanmar, Khmer,
Mongolian, Fullwidth.

**NOT covered**: ASCII (caller bitmap-tests separately by convention),
less-common scripts (Lao non-digit letters, Ogham, Tifinagh, Phoenician,
etc.), Letter Modifier / Letter Other beyond IPA, mathematical
alphanumeric symbols, Hebrew/Arabic presentation forms, surrogate
codepoints.

Picked to lift dominant test262 cases at minimum code-size cost.
A v1.0 follow-up will auto-import full UCD from `UnicodeData.txt`.

## Performance

Each lookup is a single binary search — O(log N) where N is per-property
range count (~50 for L, ~25 for N as of v0.1.0). On aarch64 M-series:

| Lookup | Hot cache median |
|---|---:|
| `is_letter_cp` (hit, CJK U+4E2D) | **2.8 ns** |
| `is_letter_cp` (hit, Greek U+03B1) | **2.3 ns** |
| `is_letter_cp` (miss, bullet U+2022) | **2.9 ns** |

(Run `cargo bench -p torajs-ucd` to reproduce; numbers above are
M-series Mac, release profile, criterion 100-sample analysis.)

Performance regression gate at `tests/perf_gate.rs`; see
[BUDGETS.md](BUDGETS.md) for the per-path budget table.

## Where this is used

- [torajs] runtime, `runtime_regex.c::cc_test_cp` calls equivalent
  C-side tables to back `\p{L}` / `\p{N}` regex matching. The C side
  will be rewritten to Rust + linked against this crate as the
  runtime rewrite progresses (per `docs/architecture-rewrite.md`).

## License

Licensed under either of

- Apache License, Version 2.0
  ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license
  ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.

[torajs]: https://github.com/goliajp/torajs
