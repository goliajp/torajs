# Changelog

All notable changes to `torajs-ucd` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-22

### Added

- Curated Unicode Letter (L) and Number (N) range tables for
  ECMAScript regex `\p{NAME}` property class lookup.
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

### Notes

- `#![no_std]` — zero allocation, zero dependencies. Pure
  binary search over static `&[(u32, u32)]` tables.
- The tables are an **intentional partial cover** of the full
  UCD Letter / Number categories — picked to lift dominant
  test262 cases at minimum code-size cost. Full UCD coverage
  would require auto-importing from `UnicodeData.txt`.
- ASCII portion (cp < 128) is **not** included; callers
  bitmap-test ASCII separately (regex-VM convention).
