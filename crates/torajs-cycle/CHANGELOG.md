# Changelog

All notable changes to `torajs-cycle` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial crate scaffold extracted from `runtime_cycle.c` (510
  LOC of C) as **P4.4** (commit `eab2435`, 2026-05-24).
- Bacon & Rajan trial-deletion cycle collector across 5 modules
  (lib + layout + buffer + collect + arr).
- Three-color scheme (BLACK / GRAY / WHITE) over buffered trial
  roots; recursive mark / scan / collect.
- Per-class child-offset table integration via ssa_inkwell-emitted
  `__torajs_class_layouts` extern.
- Auto-collect threshold + manual `gc()` trigger + main-exit
  drain.

### Polished (2026-05-25)

- README.md with badges + algorithm rationale + module layout +
  cross-tier deps + trigger conditions + scope delimiter.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- BUDGETS.md (mark / scan / collect phase budgets + buffer-fill
  threshold rationale + allocation count per cycle).
- benches/cycle.rs placeholder.
- Cargo.toml: criterion dev-dep + [[bench]] section.
