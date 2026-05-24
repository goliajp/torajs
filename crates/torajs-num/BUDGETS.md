# torajs-num performance budgets

`torajs-num` wraps libm via Rust's `f64` methods. The Math intrinsics
add **zero overhead** over libm itself — each `__torajs_math_<f>` is
a one-line forwarding fn that LTO inlines into the AOT user code. The
budgets below reflect what libm itself can deliver; we just want to
verify the wrap doesn't add measurable cost.

## Path taxonomy

| Path | Hot/Cold | Notes |
| --- | --- | --- |
| `__torajs_math_*` (sqrt, pow, sin, ...) | **Hot** | Per Math.* call in user code. Mandelbrot / prime_count loops run these millions/sec. |
| `__torajs_num_is_*` (isNaN, isFinite, isInteger, isSafeInteger) | **Hot** | Branch predicates; usually one per loop iteration in numerical code. |
| `parse_int` / `parse_float` | Warm | Per CSV row / JSON number. ~thousands/sec on data-processing workloads. |
| `Number.toString` / `toFixed` / `toExponential` | Warm | Per console.log of a number; per JSON.stringify of numeric field. |
| Heap-Number layout fns (`object_is.rs` / `print_err.rs`) | Cold | Rare paths; not budgeted. |

## Budgets

| Path | Budget | Observed (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `math_sqrt-100k` | ≤ 1 ms | ~0.5 ms | 2× | 100k sqrt calls on `i as f64` inputs. ~5 ns / call. Limited by libm sqrt itself. |
| `math_pow-10k` | ≤ 0.5 ms | ~0.2 ms | 2.5× | 10k pow calls with non-integer exponent (forces general libm path). ~20 ns / call. |
| `math_floor-100k` | ≤ 0.5 ms | ~0.2 ms | 2.5× | 100k floor calls. Cheap libm op, ~2 ns / call. |
| `parse_int-mixed-1k` | ≤ 0.5 ms | ~0.15 ms | 3× | 1k iterations × 7 mixed input shapes = 7k calls. ~20 ns each amortized (the leading-whitespace skip + sign + `0x` detect dominates). |
| `parse_float-mixed-1k` | ≤ 1 ms | ~0.3 ms | 3× | Similar shape; the parse-state machine is slightly heavier. |

## Code size

| Path | Budget | Measured |
| --- | ---: | ---: |
| `libtorajs_num.a` artifact | ≤ 100 KB | ~50 KB |
| Per-call code at the AOT site (Math intrinsic) | ≤ 4 bytes | `bl __torajs_math_<f>` = 4 bytes |
| Cumulative effect on fib40 user binary | n/a | The full crate adds ~10 KB to a Math-using user binary (most Math fns are tree-shaken out per-binary by dead_strip + LTO). |

## What's NOT budgeted

- **libm-itself's correctness or speed.** We delegate; if libm has a
  bad rounding mode for `Math.log1p` near zero, we inherit it (and
  document the bug; we don't try to "fix" libm in this crate).
- **Heap-Number alloc**: `__torajs_num_alloc_pooled` is in `torajs-rc`
  / `torajs-anyvalue` territory; the alloc cost is budgeted there.
- **`Number.toString(radix)` for arbitrary radix**: hot path is
  radix 10; non-10 paths (radix 2 / 8 / 16) use a per-digit divmod
  loop — slower but not on any bench corpus hot path.
- **Spec-compliance fixes**: documented in CHANGELOG when shipped.
  Round / Max / Min / SafeInteger boundary tests in `tests/spec_compat.rs`
  catch the corners where libm diverges from spec.
