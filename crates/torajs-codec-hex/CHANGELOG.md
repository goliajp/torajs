# Changelog

All notable changes to `torajs-codec-hex` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-25

### Added

- Initial crate scaffold extracted from torajs Phase 2 F.1 (commit
  `9b7740c`, 2026-05-25). Phase 2 of the torajs architecture goal
  ("0 Cargo deps from the metal-level core") opens with this crate
  replacing `hex 0.4` — the simplest community dep on the torajs
  dep tree (pure algorithm, no async, no features matrix).
- `encode<T: AsRef<[u8]>>(bytes: T) -> String` — lower-case
  encode. Allocates one `String` of exactly `2 * bytes.len()`
  bytes; no intermediate buffers.
- `decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, FromHexError>`
  — tolerates lower-case, upper-case, and mixed-case on each
  digit independently (matches `hex 0.4`).
- `FromHexError { OddLength, InvalidHexCharacter { c, index } }`
  — same variant shape as `hex 0.4`'s error type.
- 11 unit tests covering empty input, lower/upper/mixed case, odd
  length, invalid char (with index position), full 256-byte
  alphabet roundtrip, and a SHA-256 digest fixture.

### Polished (2026-05-25)

- README.md with badges + Quick start + compatibility section.
- CHANGELOG.md (this file) per Keep a Changelog v1.1.0.
- LICENSE-MIT + LICENSE-APACHE dual-license.
- `tests/hex_compat.rs` — byte-identical compatibility test against
  the published `hex 0.4` output on 4 fixtures (empty, basic, full
  byte alphabet, SHA-256 digest) without actually depending on
  `hex 0.4` (hardcoded expected outputs).
- `benches/codec_hex.rs` — criterion benches for encode + decode
  on the SHA-256-digest-shape input (32-byte / 64-char), the
  workspace's hot path.
- `BUDGETS.md` — encode + decode latency budgets with 10× headroom.
