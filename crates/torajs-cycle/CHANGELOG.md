# Changelog

All notable changes to `torajs-cycle` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-24

### Added

- Initial release, replacing the previous C implementation.
- Bacon & Rajan trial-deletion cycle collector across 5 modules
  (lib + layout + buffer + collect + arr).
- Three-color scheme (BLACK / GRAY / WHITE) over buffered trial
  roots; recursive mark / scan / collect.
- Per-class child-offset table integration via the
  compiler-emitted `__torajs_class_layouts` extern.
- Auto-collect threshold + manual `gc()` trigger + main-exit
  drain.
