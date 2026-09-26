# Changelog

All notable changes to `torajs-date` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial release: full ECMAScript `Date` class surface in Rust,
  replacing the previous C implementation.
- 6 source files organized by concern (layout / tm / civil /
  getters / parse / api).
- Howard Hinnant `civil_from_days` / `days_from_civil` proleptic
  Gregorian arithmetic.
- libc `localtime_r` / `mktime` for timezone-aware accessors.
