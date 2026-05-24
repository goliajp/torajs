# Changelog

All notable changes to `torajs-fetch` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial crate scaffold extracted from `runtime_fetch.c` (~340
  LOC of C) as **P6.3** (commit `f04a2dc`, 2026-05-24). Replaces
  the sync GET MVP that previously lived as a separate C file.
- `__torajs_fetch_sync(url) -> *mut c_void` — synchronous HTTP
  GET via libcurl-easy. Returns a Response heap block on success
  (status + body Str); empty-body Response with status 0 on
  network errors.
- `__torajs_response_drop(p)` — decrement body Str refcount + free
  the Response block.
- Native target uses libcurl (`#[link(name = "curl")]`); wasm32-
  wasi target degrades to empty-body Response shape.

### Polished (2026-05-25)

- README.md with badges + Response layout diagram + ABI docs +
  "What's NOT in scope" delimiter for v0.1.0 limitations (HTTP
  methods / headers / TLS / async / etc.).
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `benches/fetch.rs` placeholder (real fetch is network-state-
  dependent; integration tests via end-to-end conformance gate).
- `BUDGETS.md` — documents that latency is dominated by network
  RTT; the per-call libcurl-easy setup is well under 1 ms.
