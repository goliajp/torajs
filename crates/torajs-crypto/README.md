# torajs-crypto

Cryptographic primitives for `no_std` Rust with no dependencies and no
allocator.

- SHA-256, SHA-384, SHA-512 (FIPS 180-4), streaming and one-shot
- HMAC (RFC 2104) over any of them, with constant-time tag verification
- HKDF extract and expand (RFC 5869)
- AES-128-GCM, AES-192-GCM and AES-256-GCM (NIST SP 800-38D), with an
  AArch64 path on the AES and PMULL instructions and an x86-64 path on
  AES-NI and PCLMULQDQ
- ChaCha20-Poly1305 (RFC 8439), with an AArch64 NEON keystream
- constant-time comparison and selection, and a wiping routine for secrets
- an `EntropySource` trait for callers to supply randomness
- a `CpuFeatures` value for callers to supply detected CPU capabilities

## Usage

```rust
use torajs_crypto::Digest;
use torajs_crypto::hkdf::Hkdf;
use torajs_crypto::hmac::Hmac;
use torajs_crypto::sha2::{Sha256, Sha384, sha256};

// hashing
let digest = sha256(b"hello");
let streamed = Sha256::new().chain(b"hel").chain(b"lo").finish();
assert_eq!(digest, streamed);

// HMAC
let tag = Hmac::<Sha384>::mac(b"key", b"message");
assert!(Hmac::<Sha384>::new(b"key").chain(b"message").verify(&tag).is_ok());

// HKDF
let prk = Hkdf::<Sha256>::extract(b"salt", b"input keying material");
let mut okm = [0u8; 32];
prk.expand(b"context", &mut okm).unwrap();

// AEAD, in place on a caller-owned buffer
use torajs_crypto::Aead;
use torajs_crypto::cpu::CpuFeatures;
use torajs_crypto::gcm::Aes128Gcm;

let key = Aes128Gcm::new(&[0x42; 16], CpuFeatures::compile_time());
let mut record = *b"hello";
let tag = key.seal(&[0; 12], b"header", &mut record).unwrap();
key.open(&[0; 12], b"header", &mut record, &tag).unwrap();
assert_eq!(&record, b"hello");
```

## Design

The crate performs no I/O and has no global state. It does not read a
clock, a random source, or CPU identification registers. Anything that
depends on the environment is passed in:

- **Randomness.** Functions that need random bytes take
  `&mut impl EntropySource`. A failing source returns `EntropyError` to the
  caller; the crate never aborts.
- **CPU features.** `CpuFeatures` is the value hardware code paths take
  to choose an implementation. `CpuFeatures::portable()` and
  `CpuFeatures::compile_time()` are safe; adding a feature bit is `unsafe`,
  because claiming a feature the CPU lacks would execute an unsupported
  instruction. The AES-GCM constructors take one: with both
  `Aarch64Aes` and `Aarch64Pmull` set they use the ARMv8 AES and PMULL
  instructions, with `X86Aes`, `X86Pclmulqdq` and `X86Ssse3` set the
  x86-64 AES-NI and PCLMULQDQ instructions, otherwise the portable code. The ChaCha20-Poly1305
  constructor takes one too and runs the keystream on NEON when
  `Aarch64Neon` is set. With `Aarch64Dit` set, both AEADs switch on
  Arm data-independent timing (`PSTATE.DIT`) for key setup, sealing and
  opening, and restore the previous state afterwards;
  `cpu::with_data_independent_timing` does the same around any closure.
  The SHA-256 ARMv8 path is
  still selected at compile time (`target_feature = "sha2"`).

All buffers are provided by the caller. Nothing allocates.

## Security model

- Operations on secret data (keys, MAC states, derived secrets) run in time
  that depends only on public lengths: no secret-dependent branches,
  indices or early-exit comparisons. Comparisons and selections go through
  the `ct` module.
- No secret indexes a table. The portable AES is bitsliced (the S-box is a
  Boolean circuit evaluated on all bytes at once) and the portable GHASH
  multiplies with integer instructions on masked operands.
- Tag verification (`Hmac::verify`, `Hmac::verify_truncated`, AEAD `open`)
  compares in constant time. A failed `open` overwrites the caller's buffer
  with zeros, so unauthenticated plaintext never reaches the caller.
- Types that hold key material (`Hmac`, `Hkdf`, the AEAD key types) wipe
  it on drop with volatile writes, so the wipe is not optimized away.
  Named stack buffers that hold keystream or GHASH state are wiped the same
  way; copies of registers the compiler spills to the stack are not.
- `Debug` output of state and key types prints the type name only.

`unsafe` is denied crate-wide. Unsafe code exists in the wiping routine
(`ct::zeroize`) and in the hardware modules: the AArch64 SHA-256 block
function, compiled only when the target enables the `sha2` feature at build
time, the AArch64 AES and GHASH code, reached only through a `CpuFeatures`
value that vouches for the AES and PMULL instructions, the x86-64 AES and
GHASH code, reached only through one that vouches for AES-NI, PCLMULQDQ and
SSSE3, the NEON ChaCha20 code, reached only through one that vouches for
NEON, and the `PSTATE.DIT` register accesses, reached only through one that
vouches for `FEAT_DIT`.
`CpuFeatures::with` is declared `unsafe fn` as a caller contract and
contains no unsafe code.

## Non-goals

- Protection against power analysis, fault injection, or speculative
  execution attacks.
- Legacy algorithms (MD5, SHA-1).
- Runtime CPU feature detection; that belongs to the caller.

## Testing

Unit tests cover the FIPS 180-4 examples, all RFC 4231 HMAC test cases and
all RFC 5869 SHA-256 HKDF test cases, the FIPS 197 AES examples, the
SP 800-38D GCM examples and the RFC 8439 ChaCha20 / Poly1305 / AEAD examples.
`tests/wycheproof.rs` and `tests/wycheproof_aead.rs` run the Project
Wycheproof HMAC, HKDF, AES-GCM and ChaCha20-Poly1305 vectors (the JSON files
under `tests/data` come from Project Wycheproof and are licensed under
Apache-2.0); AES-GCM and ChaCha20-Poly1305 vectors run on both the portable
and the hardware path. A unit test compares the two AES-GCM paths on 100 000
random inputs, another the two ChaCha20 keystreams on every length up to
1100 bytes and some longer ones. Build
with `RUSTFLAGS="--cfg torajs_crypto_portable"` to test the portable code on
hardware that would otherwise take an accelerated path.

Two ignored test files hold measurements: `tests/dudect.rs` (statistical
constant-time checks after dudect) and `tests/throughput.rs` (sealing 16 KiB
records).

## Minimum supported Rust version

Rust 1.88.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
