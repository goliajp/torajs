# Changelog

All notable changes to `torajs-process` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial crate scaffold extracted from `runtime_str.c`'s process
  family (~240 LOC) as **P7.h-proc** (commit `5527de7`, 2026-05-24).
- `__torajs_process_exit` / `_cwd` / `_env_get` / `_argv` /
  `_platform` / `_stdout_write` / `_stderr_write`.

### Polished (2026-05-25)

- README.md with badges + Node.js-compatible surface table +
  "What it does NOT do" delimiter for v0.1.0.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `BUDGETS.md` — syscall-dominated; Rust wrapper overhead < 100 ns.
- `benches/process.rs` placeholder.
- Cargo.toml: criterion dev-dep + [[bench]] section.
