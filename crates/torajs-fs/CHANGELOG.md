# Changelog

All notable changes to `torajs-fs` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial crate scaffold extracted from `runtime_str.c`'s `fs_*`
  family (~340 LOC) as **P7.d** (commit `88c21c4`, 2026-05-24).
- `__torajs_fs_read_file_sync` / `_write_file_sync` /
  `_append_file_sync` / `_exists_sync` / `_mkdir_sync` /
  `_unlink_sync` / `_stat_size_sync` / `_readdir_sync`.

### Polished (2026-05-25)

- README.md with badges + Node.js-compatible surface table +
  "What it does NOT do" delimiter for v0.1.0.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `BUDGETS.md` — latency dominated by syscall + page-cache RTT;
  Rust wrapper overhead is < 100 ns per call.
- `benches/fs.rs` placeholder (real fs latency depends on disk
  cache state; covered via end-to-end conformance gate).
- Cargo.toml: criterion dev-dep + [[bench]] section.
