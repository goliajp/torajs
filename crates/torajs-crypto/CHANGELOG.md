# Changelog

All notable changes to `torajs-crypto` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `sha2`: SHA-256, SHA-384 and SHA-512 (FIPS 180-4), streaming types and
  one-shot functions; the SHA-256 block function uses the ARMv8 SHA-256
  instructions when the target enables `sha2` at build time.
- `Digest` trait (sealed) shared by the SHA-2 types.
- `hmac`: `Hmac<D>` with constant-time `verify` and `verify_truncated`.
- `hkdf`: `Hkdf<D>` with `extract`, `from_prk`, `expand` and
  `expand_parts`.
- `ct`: `Choice`, `ct_eq`, `ct_select`, `cmov`, and `ct::zeroize`.
- `rand`: the `EntropySource` trait and `EntropyError`.
- `cpu`: `CpuFeatures` and `Feature`, including `Feature::Aarch64Neon`,
  `Feature::Aarch64Dit`, `Feature::Aarch64Sb` and `Feature::X86Ssse3`.
- `cpu::with_data_independent_timing`: runs a closure with Arm
  `PSTATE.DIT` set (followed by a speculation barrier) and restores the
  previous value; nested scopes leave the bit as they found it. The AEAD
  keys enter the same scope for key setup, `seal` and `open` when their
  `CpuFeatures` vouches for `Feature::Aarch64Dit`.
- `aead`: the sealed `Aead` trait (`seal` / `open` in place, 96-bit nonce,
  128-bit tag), `AeadError`, `NONCE_LEN`, `TAG_LEN`.
- `chacha20poly1305`: `ChaCha20Poly1305` (RFC 8439). The constructor takes
  a `CpuFeatures`; on AArch64 with NEON the keystream runs on vector
  registers, twelve blocks at a time on bulk data.
- `gcm`: `Aes128Gcm`, `Aes192Gcm`, `Aes256Gcm` (NIST SP 800-38D), with
  `seal_with_iv` / `open_with_iv` for IVs of any non-zero length. The
  constructors take a `CpuFeatures`; on AArch64 with AES and PMULL, and on
  x86-64 with AES-NI, PCLMULQDQ and SSSE3, the hardware path is used,
  otherwise bitsliced AES and a table-free GHASH.
