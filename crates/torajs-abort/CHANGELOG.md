# Changelog

All notable changes to `torajs-abort` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial release, replacing `expect()` / `panic!()` / `assert!()`
  call sites in staticlib crates with `abort_with(b"msg")`,
  stripping the Rust `std::panic` infrastructure (gimli / addr2line
  / rustc_demangle / std::backtrace / std::io::Error /
  std::thread::Thread) from every user binary.
- `abort_with(msg: &[u8]) -> !` — Rust-ergonomic call site.
- `__torajs_abort_with(*const u8, usize) -> noreturn` — `extern "C"`
  symbol for C / cross-staticlib link-time resolution.
- `#![no_std]` — 0 Cargo deps; `unsafe extern "C"` declares
  `write(2)` + `abort()` directly.
- No inter-crate dependency — any staticlib may use it.
