# Changelog

All notable changes to `torajs-ucd` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-22

### Added

- Initial crate scaffold extracted from the torajs AOT
  TypeScript runtime (`crates/torajs-runtime/src/runtime_regex.c`
  UCD section, P9.3-A2 ship 2026-05-19). Provides curated
  Unicode Letter (L) and Number (N) range tables for ECMAScript
  regex `\p{NAME}` property class lookup.
- `UCD_LETTER` static range table (~50 entries): Latin-1
  supplement, IPA + Spacing Modifier, Greek + Coptic, Cyrillic,
  Armenian, Hebrew, Arabic, Devanagari, Thai, Hiragana,
  Katakana, CJK Unified Ideographs (basic + ext A), Hangul
  Syllables.
- `UCD_NUMBER` static range table (~25 entries): Latin-1
  numeric, Arabic-Indic, NKo, Devanagari/Bengali/Gurmukhi/
  Gujarati/Oriya/Tamil/Telugu/Kannada/Malayalam/Sinhala/Thai/
  Lao/Tibetan/Myanmar/Khmer/Mongolian digits, Fullwidth digits.
- `range_contains(table, cp) -> bool` — binary-search membership
  test against a sorted, non-overlapping `(lo, hi)` range table.
- `is_letter_cp(cp) -> bool` / `is_number_cp(cp) -> bool` —
  convenience wrappers over `range_contains` against the
  shipped tables.
- 13 unit tests covering CJK / Hiragana / Greek / Hebrew /
  Arabic / Hangul / Devanagari / Arabic-Indic / Fullwidth +
  ASCII exclusion + boundary + sorted-invariant.
- 5 criterion micro-benches: hit-CJK / hit-Greek / miss for
  Letter; hit-Arabic-Indic / miss for Number.
- `tests/perf_gate.rs` regression gate with 50 ms budget for
  ~63k mixed lookups (≈ 3× headroom over observed P95).

### Notes

- `#![no_std]` — zero allocation, zero dependencies. Pure
  binary search over static `&[(u32, u32)]` tables.
- The tables are an **intentional partial cover** of the full
  UCD Letter / Number categories — picked to lift dominant
  test262 cases at minimum code-size cost. Full UCD import
  (auto-generated from `UnicodeData.txt`) tracked as a v1.0
  follow-up.
- ASCII portion (cp < 128) is **not** included; callers
  bitmap-test ASCII separately (regex-VM convention).
