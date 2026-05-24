# Changelog

All notable changes to `torajs-abort` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial crate scaffold extracted from torajs polish A3 (commit
  `ab7a286`, 2026-05-24). That commit replaced 13 `expect()` /
  `panic!()` / `assert!()` call sites across the Layer-1+
  staticlibs with `abort_with(b"msg")`, stripping the Rust
  `std::panic` infrastructure (gimli / addr2line /
  rustc_demangle / std::backtrace / std::io::Error /
  std::thread::Thread) from every user binary.
- `abort_with(msg: &[u8]) -> !` — Rust-ergonomic call site.
- `__torajs_abort_with(*const u8, usize) -> noreturn` — `extern "C"`
  symbol for C / cross-staticlib link-time resolution.
- `#![no_std]` — 0 Cargo deps; `unsafe extern "C"` declares
  `write(2)` + `abort()` directly.
- Layer-0 substrate per `torajs/docs/architecture-rewrite.md` —
  no inter-crate dep; all Layer-1+ staticlibs may use it.

### Measured

`fib40` user binary on `aarch64-apple-darwin` (release build):

- Pre A3: 445 KB (vanilla `cargo build --release`)
- Post A3 + A2 dead-strip: 410 KB
- Post A3 + A4.1 (build-std + panic=abort): 351 KB

The A3 contribution is the ~35 KB delta from stripping the panic
chain in the staticlib crates; the rest is dead_strip + build-std.

### Polished (2026-05-25)

- README.md with badges + Quick start + "When to use" + ABI section.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `tests/abort_smoke.rs` — black-box smoke test (subprocess
  fork + assert non-zero exit + stderr capture).
- `benches/abort.rs` — criterion bench measuring the cold-path
  `bl __torajs_abort_with` setup cost (instrumented around a
  subprocess fork to keep the timing-host process alive).
