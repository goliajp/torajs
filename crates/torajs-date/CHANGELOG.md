# Changelog

All notable changes to `torajs-date` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial crate scaffold extracted from `runtime_date.c` (~590
  LOC of C) as **P6.4** (commit `243e665`, 2026-05-24). Full
  ECMAScript `Date` class surface in Rust.
- 6 source files organized by concern (layout / tm / civil /
  getters / parse / api).
- Howard Hinnant `civil_from_days` / `days_from_civil` proleptic
  Gregorian arithmetic.
- libc `localtime_r` / `mktime` for timezone-aware accessors.

### Polished (2026-05-25)

- README.md with badges + surface table + module-layout table +
  ABI invariants + "What's NOT in scope" delimiter.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- BUDGETS.md (civil-arithmetic + parse + libc localtime latency).
- benches/date.rs placeholder (integration tests via conformance).
- Cargo.toml: criterion dev-dep + [[bench]] section.
