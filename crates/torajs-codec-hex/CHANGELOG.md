# Changelog

All notable changes to `torajs-codec-hex` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-05-25

### Added

- Initial release: byte-identical drop-in replacement for the
  `hex 0.4` community crate.
- `encode<T: AsRef<[u8]>>(bytes: T) -> String` — lower-case
  encode. Allocates one `String` of exactly `2 * bytes.len()`
  bytes; no intermediate buffers.
- `decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, FromHexError>`
  — tolerates lower-case, upper-case, and mixed-case on each
  digit independently (matches `hex 0.4`).
- `FromHexError { OddLength, InvalidHexCharacter { c, index } }`
  — same variant shape as `hex 0.4`'s error type.
