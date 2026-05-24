# torajs-bigint

[![Crates.io](https://img.shields.io/crates/v/torajs-bigint?style=flat-square&logo=rust)](https://crates.io/crates/torajs-bigint)
[![docs.rs](https://img.shields.io/docsrs/torajs-bigint?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-bigint)
[![License](https://img.shields.io/crates/l/torajs-bigint?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-bigint?style=flat-square)](https://crates.io/crates/torajs-bigint)

Self-hosted arbitrary-precision integer (`BigInt`) substrate for the
[torajs] AOT TypeScript runtime. 0 Cargo deps. Implements the full
ECMAScript `BigInt` spec surface — arithmetic, bitwise, shift,
comparison, decimal / hex / i64 parsing, and `.toString(radix)` —
in 2.2 KLOC of pure Rust.

Replaces ~1.3 KLOC of `runtime_bigint.c` (closed P3.3, commit
`0ba388a`, 2026-05-23) — the FIRST major C substrate to go fully
to pure Rust, validating the architecture-rewrite pattern that
later expanded to `runtime_promise.c` / `runtime_regex.c` /
`runtime_*.c`.

## Algorithm choices

| Op | Algorithm | Source |
| --- | --- | --- |
| Repr | Sign + little-endian `u64`-limb magnitude | Standard PL textbook |
| Add / Sub | Limb-by-limb with carry propagation | CPython `_PyLong_Add` / `_PyLong_Sub` shape |
| Mul | Schoolbook for ≤ 32-limb operands; Karatsuba for larger | GMP threshold lore |
| Div / Mod | Knuth Algorithm D (long division with normalization) | TAoCP Vol 2 §4.3.1 |
| Pow | Square-and-multiply | Standard |
| Bitwise / Shift | Two's-complement view via `Wrapping<u64>` | ECMAScript §6.1.6.2 |
| `.toString(10)` | Repeated divmod by 10^19 (limb-sized batch) | Standard |
| `.toString(16)` / `2` | Direct limb iter + bit shifts | Standard |
| Parse decimal | Limb-by-limb mul-add accumulation | Standard |
| Parse hex | Direct nibble-pack into limbs | Standard |

## Module layout (`src/`)

| File | Purpose |
| --- | --- |
| `layout.rs` | Heap-block layout (universal header + sign byte + len + limb array). |
| `construct.rs` | Constructors (from i64, decimal str, hex str, clone). |
| `arith.rs` | Add + Sub at sign-aware level. |
| `mul.rs` | Schoolbook + Karatsuba multiply. |
| `divmod.rs` | Knuth long division + helpers. |
| `bitwise.rs` | AND / OR / XOR / NOT in two's-complement view. |
| `shift.rs` | Left / right shift with sign extension. |
| `compare.rs` | Equality + ordered comparison. |
| `tostring.rs` | `.toString(radix)`. |
| `internal.rs` | Limb-level helpers shared across all modules. |
| `str_bridge.rs` | Cross-tier str_alloc_pooled FFI for tostring output. |
| `drop.rs` | rc_dec + free integration. |

## Quick start

The crate is consumed exclusively through its `extern "C"` ABI by
the AOT-emitted user code. Rust callers can also use the public
Rust API for unit testing:

```rust
use torajs_bigint::*;

// Constructed via decimal-string parse (the spec `BigInt("...")`
// path).
unsafe {
    let a = __torajs_bigint_from_decimal(b"123456789012345678901234567890\0".as_ptr());
    let b = __torajs_bigint_from_decimal(b"987654321098765432109876543210\0".as_ptr());
    let sum = __torajs_bigint_add(a, b);
    // ... use sum ...
    __torajs_bigint_drop(sum);
    __torajs_bigint_drop(b);
    __torajs_bigint_drop(a);
}
```

## Spec compliance

- **Sign of zero**: BigInt has no signed zero; `0n + (-0n)` is `0n`.
  Internally, the sign byte is always 0 when the magnitude is empty.
- **Division by zero**: throws `RangeError` via `torajs-throw`'s
  `__torajs_throw_range_error` cross-tier extern.
- **`>>`** is arithmetic (sign-extending), `>>>` is rejected at parse
  time per spec (BigInt has no unsigned-shift).
- **Bitwise on negative**: two's-complement view — i.e. `-1n & 5n == 5n`,
  `~0n == -1n`. Implemented via temporary u64-limb two's-complement
  buffer + sign-aware result reconstruction.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
