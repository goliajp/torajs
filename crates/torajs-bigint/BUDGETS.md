# torajs-bigint performance budgets

`torajs-bigint` doesn't sit on any benchmark's hot loop in the torajs
default bench corpus (none of the 26 bench cases use BigInt
arithmetic in a tight loop). The budgets here are documentary,
calibrated against representative cryptographic-style workloads
(~256-bit numbers) — where Karatsuba threshold for multiplication is
~32 limbs (≈ 256 bits at 64 bits/limb), and divmod's Knuth-D is
dominated by the per-quotient-digit estimate-and-refine cost.

## Path taxonomy

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| `add` / `sub` | Hot | Per arithmetic op; the most common. |
| `mul` (schoolbook) | Warm | ≤ 32-limb operands. |
| `mul` (Karatsuba) | Warm | > 32-limb operands; recursion depth log₂(n_limbs / 32). |
| `divmod` | Warm | Per `%` or `/`; Knuth long-division. |
| `to_string(10)` | Warm | Per JSON.stringify / console.log of a BigInt; repeated divmod by 10^19. |
| `from_decimal` / `from_hex` | Warm | Per BigInt("...") call. |
| `bitwise` (and / or / xor / not) | Warm | Two's-complement view. |

## Budgets

| Path | Budget | Observed (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `bigint_add-256bit-10k` | ≤ 5 ms | ~2 ms | 2.5× | 10k adds of 256-bit numbers. Each is a 4-limb-pair carry-propagation loop + a heap alloc for the result. The alloc dominates. |
| `bigint_mul-256bit-1k` | ≤ 5 ms | ~2 ms | 2.5× | 1k schoolbook 4×4-limb multiplies. The schoolbook path is ~16 u128 mul-adds + carry-chain. |
| `bigint_div-256bit-1k` | ≤ 20 ms | ~8 ms | 2.5× | 1k Knuth-D long divisions of ~68-digit dividend by 15-digit divisor. ~5 quotient digits each. |

## GMP comparison (informational)

For the 256-bit numbers above, GMP's `mpz_mul` is typically 3-5×
faster than schoolbook torajs-bigint because it switches to
Toom-Cook earlier (~16 limbs) and uses SIMD-aware inner-loop
unrolling. We don't target GMP parity at v0.1.0; the algorithmic
choices are textbook + appropriate for the workspace's actual
workload (mostly arithmetic on i64-sized values; large-number
crypto is out of scope for torajs's TS surface).

## Code size

| Path | Budget | Measured |
| --- | ---: | ---: |
| `libtorajs_bigint.a` artifact | ≤ 200 KB | ~150 KB |
| Per-call code at the AOT site | ≤ 4 bytes | `bl __torajs_bigint_<op>` = 4 bytes |
| Cumulative effect on a non-BigInt-using user binary | 0 bytes | LTO + dead_strip remove the unreferenced symbols. |

## What's NOT budgeted

- **Toom-Cook / FFT-based multiplication** for very-large numbers
  (> 1024 bits). Out of scope for v0.1.0; if a future caller has
  a large-number workload we'd evaluate adding a Toom-Cook tier.
- **Montgomery / Barrett modular reduction** for cryptographic
  modular exponentiation. Not in torajs's TS surface today.
- **Constant-time operations**. `torajs-bigint` is NOT a cryptographic
  primitive; do not use it for key material.
