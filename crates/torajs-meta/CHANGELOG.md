# Changelog

All notable changes to `torajs-meta` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial crate scaffold extracted from `runtime_str.c`'s fnprops +
  class / proto + reflect families (~510 LOC) as **P7.g**
  (commit `a4093e8`, 2026-05-24).
- `fnprops.rs` — side-table mapping function instances to property
  bags (for `fn.foo = "bar"; fn.foo` user-code patterns where
  closures need extra attached state beyond their environment
  capture).
- `classmeta.rs` — class-tag-keyed registries: class name, prototype
  object, instanceof check via prototype chain walk.
- `reflect.rs` — `Object.getOwnPropertyDescriptor` /
  `getPrototypeOf` / `setPrototypeOf` reflection ops.

### Polished (2026-05-25)

- README.md with badges + module-layout table + cross-tier deps
  + "What's NOT in scope" delimiter.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `BUDGETS.md` — lookup-table hit + miss latencies for each of the
  three concerns; reflection ops are warm but not on any bench
  corpus hot path.
- `benches/meta.rs` placeholder.
- Cargo.toml: criterion dev-dep + [[bench]] section.
