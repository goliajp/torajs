# Changelog

All notable changes to `torajs-panic` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial crate scaffold extracted from torajs P7.i-panic
  (commit `7af88d6`, 2026-05-24). Replaces the `__torajs_panic`
  helper previously living in `runtime_str.c` (the file was
  reduced to a 40-line comment-only stub at P7.i-closer and
  deleted at A.1).
- `__torajs_panic(msg: *const c_char) -> !` — writes the
  message to stderr (one `fputs` call) + emits a symbolicated
  backtrace (macOS atos / Linux raw PC chain) + `exit(101)`.
- macOS executable-path lookup via `_NSGetExecutablePath`
  (Mach-O API).
- Linux backtrace fallback via libc `backtrace` +
  `backtrace_symbols` (no `gimli` / `addr2line` dep).

### Polished (2026-05-25)

- README.md with badges + "Difference from `torajs-abort`" table
  + symbolication strategy section + "What it does NOT do".
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `tests/exit_code.rs` — fork-based smoke test verifying the
  process exits with code 101 after a `__torajs_panic` call +
  that the message lands on stderr.
- `BUDGETS.md` — documents that latency is irrelevant (process
  exits anyway) and the only real metric is the static memory
  footprint of the backtrace symbol-table.
