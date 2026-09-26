# Changelog

All notable changes to `torajs-bigint` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-23

### Added

- Initial release, replacing the previous C implementation.
- 12 source modules organized by op family (arith / bitwise / compare
  / construct / divmod / drop / mul / shift / tostring + internal /
  layout / str_bridge utilities).
- Sign-and-magnitude layout with u64-limb little-endian magnitude.
- Two's-complement view for bitwise / shift ops.
- Cross-tier RangeError throw for division-by-zero via
  `torajs-throw`'s `__torajs_throw_range_error`.
