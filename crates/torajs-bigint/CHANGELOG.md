# Changelog

All notable changes to `torajs-bigint` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-23

### Added

- Initial crate scaffold replacing `runtime_bigint.c` (~1.3 KLOC).
  P3.3 ship sequence:
  - P3.3-a (`6c4afc6`): scaffold + drop/drop_rc.
  - P3.3-b (`8c3bc98`): construct (decimal / hex / str / i64 / clone).
  - P3.3-c (`064c542`): arith add / sub + magnitude helpers.
  - P3.3-d (`c89ff66`): mul (schoolbook + Karatsuba).
  - P3.3-e (`1c7d7a5`): divmod + pow + neg + bit helpers.
  - P3.3-f (`aaf9bac`): compare / eq.
  - P3.3-g (`6c9d190`): to_string + str_bridge + divmod_chunk.
  - P3.3-h (`dd4fa29`): bitwise (AND / OR / XOR / NOT).
  - P3.3-i (`0ba388a`): shift + from_number + delete C runtime_bigint.c.
- 12 source modules organized by op family (arith / bitwise / compare
  / construct / divmod / drop / mul / shift / tostring + internal /
  layout / str_bridge utilities).
- Sign-and-magnitude layout with u64-limb little-endian magnitude.
- Two's-complement view for bitwise / shift ops.
- Cross-tier RangeError throw for division-by-zero via
  `torajs-throw`'s `__torajs_throw_range_error`.

### Polished (2026-05-25)

- README.md with badges + algorithm table + module-layout table
  + Quick start + spec-compliance notes.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `tests/spec_cases.rs` — black-box tests for the spec-correctness
  corners: zero-sign normalization, two's-complement bitwise on
  negatives, shift sign-extension, decimal / hex parse round-trip.
- `benches/bigint.rs` — criterion benches on add / mul / divmod
  at workloads representative of cryptographic-style numerical code
  (~256-bit numbers, the threshold at which Karatsuba kicks in).
- `BUDGETS.md` — per-op latency budgets cross-referenced against
  GMP for the cases where torajs-bigint is a wrap.
