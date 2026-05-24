# torajs-codec-hex

[![Crates.io](https://img.shields.io/crates/v/torajs-codec-hex?style=flat-square&logo=rust)](https://crates.io/crates/torajs-codec-hex)
[![docs.rs](https://img.shields.io/docsrs/torajs-codec-hex?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-codec-hex)
[![License](https://img.shields.io/crates/l/torajs-codec-hex?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-codec-hex?style=flat-square)](https://crates.io/crates/torajs-codec-hex)

Tiny lower-case hex encode / decode codec — drop-in replacement for the
`hex 0.4` community crate. 0 Cargo deps. Byte-identical output to
`hex::encode` / `hex::decode` for the surface we expose.

Extracted from the [torajs] AOT TypeScript runtime as **F.1** (the
first sub-step of Phase 2's "0 deps" effort, commit `9b7740c`,
2026-05-25). The torajs vision priority #4 is "0 Cargo deps from the
metal-level core"; `hex 0.4` was the easiest dep to displace — pure
algorithm, no async surface, no Cargo-features matrix, no platform
gates. F.1 ships the pattern (per-crate, zero-dep, byte-identical,
lean public API) that the rest of the F-series (`torajs-hash`,
`torajs-error`, `torajs-codec-toml`, ...) will follow.

## Quick start

```rust
use torajs_codec_hex::{decode, encode, FromHexError};

let bytes = b"foo";
assert_eq!(encode(bytes), "666f6f");

assert_eq!(decode("666f6f").unwrap(), b"foo");
assert_eq!(decode("666F6F").unwrap(), b"foo"); // upper-case tolerated

assert_eq!(decode("abc"), Err(FromHexError::OddLength));
assert_eq!(
    decode("zz"),
    Err(FromHexError::InvalidHexCharacter { c: 'z', index: 0 })
);
```

## API surface (deliberately lean)

```rust
pub fn encode<T: AsRef<[u8]>>(bytes: T) -> String;
pub fn decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, FromHexError>;

pub enum FromHexError {
    OddLength,
    InvalidHexCharacter { c: char, index: usize },
}
```

That's the whole crate. Upper-case encode, streaming variants,
constant-time variants, no_std + alloc — none of these ship initially.
Add when a caller actually needs them.

## Compatibility with `hex 0.4`

- `encode`: byte-identical for all valid `AsRef<[u8]>` inputs.
- `decode`: byte-identical for all valid hex strings; tolerates
  lower-case, upper-case, and mixed-case on each digit independently
  (matches `hex 0.4` semantics).
- `FromHexError`: same variant names + payload shapes as `hex 0.4`'s
  `FromHexError::OddLength` and `FromHexError::InvalidHexCharacter`.
  The crate intentionally does NOT re-export `hex 0.4` itself; the
  error type is a fresh enum with the same shape.

## What's NOT in scope

- **Constant-time decode**: this crate is a workspace utility for cache
  keys / hash digest formatting / config encoding. It is NOT a
  cryptographic primitive. For constant-time hex (e.g. to avoid
  side-channel leaks on key material), use a dedicated crate.
- **Streaming / iterator API**: every byte fits in a single `String`
  allocation; the workspace's actual usage is SHA-256 digest hex (64
  ASCII chars) where streaming buys nothing.
- **`hex_literal!` macro**: not needed; the workspace's hex use is
  data-dependent at runtime.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
