# array-map-1m

10M-element `Array.map` through a capturing closure, then iter-sum the result.

## Workload

```ts
function loopSum(n: number, k: number): number {
  let xs: number[] = [];
  for (let i = 0; i < n; i++) xs.push(i);

  // capturing-closure map: ys[i] = xs[i] + k
  let ys: number[] = xs.map((x: number): number => x + k);

  let sum: number = 0;
  for (let i = 0; i < ys.length; i++) sum = sum + ys[i];
  return sum;
}

console.log(loopSum(10_000_000, 2));   // 50000015000000
```

## Why this case

Validates M6.2 end-to-end: `xs.map(closure)` lowered to a header-body-after loop that calls a heap-env closure on each element and pushes the result onto a freshly-allocated output array. Per element: load element → load fn_ptr from env+0 → indirect call with env as arg 0 → append to dst. The append used to be a cross-archive call; since rotation 534 it is a slot store and an add (see the result section).

Rust uses `Box<dyn Fn>` + `black_box(&dyn Fn)` + `#[inline(never)]` to defeat devirtualization; without those, rustc collapses the entire fn-call indirection. Go uses a non-inlined fn + a closure literal — same shape. Both then `iter().map().collect::<Vec<_>>()` (rust) / `for _, x := range xs { ys = append(ys, add(x)) }` (go) which are heavily-optimized library paths.

## Result on M4 Pro (2026-08-30, bench-harness `--runs 5`)

```
torajs (AOT)    29.76 ms   ← 1.72× faster than bun
bun-aot         51.20 ms
rust            20.36 ms
torajs-run      67.00 ms
```

### rotation 534 — the append stopped leaving the function

`map` and `filter` reserve the destination in the preheader, then used to
pay a cross-archive `bl __torajs_arr_push_unchecked` per element: a call,
a return, and inside it a load and a store of the cell's own length word,
all on the element chain. The reservation is exactly the proof an inline
append wants, so the lowering now takes it —
`ssa_lower_arr_prereserve::emit_prereserved_{state,push,len_writeback}`.

Same-session A/B, five interleaved passes each side:

| runtime | before | after | Δ |
|---|---:|---:|---:|
| **torajs (AOT)** | 37.95 (σ 0.26) | **29.76 (σ 0.03)** | **−21.6%** |
| torajs-run | 75.12 | 67.00 | −10.8% |
| bun-aot *(control)* | 50.82 | 51.20 | +0.7% |
| rust *(control)* | 20.52 | 20.36 | −0.8% |

Both controls moved under 1%, so the machine did not drift under the
measurement; `array-sum-1m`, which this change does not touch, read
20.74 → 20.77. The inner loop is now 13 instructions with no call in it:
the destination's data base and head offset hoisted into `x27` / `x28`,
the running length live in `x21`, and the append a single
`str d19, [x28, x15]`.

The one shape that cannot take it is a `map` whose product is boxed —
that is the only shape a hole can be marked in, and marking one reads the
length word this form deliberately leaves stale until the loop exits.

## Historical

Before rotation 534, with the M6.2 one-shot reserve only:

```
torajs (AOT)   31.42 ms   ← parity with rust (was 37.49 before the reserve)
rust           31.56 ms
go             25.71 ms
bun-jsc        62.89 ms
bun-aot        63.16 ms
node-v8       280.85 ms
```

Go led slightly there; its slice header lives by-value on the stack and
the compiler bulk-vectorizes the per-element write, while torajs and rust
both go through a heap-resident `(len, cap, data[])` block.
