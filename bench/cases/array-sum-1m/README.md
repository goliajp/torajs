# array-sum-1m

Build an array via 10M `push` calls (with amortized realloc), then sum it via index access.

## Workload

```ts
let xs: number[] = [];
let i: number = 0;
while (i < 10_000_000) {
  xs.push(i);
  i = i + 1;
}
let sum: number = 0;
let j: number = 0;
while (j < xs.length) {
  sum = sum + xs[j];
  j = j + 1;
}
console.log(sum);  // 49999995000000
```

Two phases:
1. **Build phase**: 10M `xs.push(i)`. Amortized O(1) per push but with O(log N) realloc events. Stresses heap allocator + memcpy on grow.
2. **Read phase**: 10M `xs[j]` reads + sum. Tight loop, one load and one add per iteration — **f64 today**, see the result section for why.

## Why this case

Validates M1.2 — `Array<T>` runtime end-to-end, and doubles as the perf
case where JS number semantics cost the most. Specifically:
- `[]` empty literal uses the let-decl's array annotation for the element type
- `xs.push(v)` lowering: the loop's pre-reserve makes it a slot store and an add, no call at all (`ssa_lower_arr_prereserve`); the write-back the old shape needed is retired (B1 — reserve never moves the cell)
- `xs.length` direct load at offset 0
- `xs[j]` `LoadDyn` SSA inst → `base + 16 + j*8`
- End-of-scope drop fires `__torajs_arr_drop` (verified via `leaks --atExit`)

## Languages

All five — torajs / rust / go / bun / node / python. Rust uses `black_box` on the field read to defeat constant folding (otherwise `-O3` collapses the entire loop into a constant since the workload is purely deterministic).

## The two phases are not the same program in every language

`main.rs` uses `Vec<i64>` and an `i64` accumulator; `main.go` uses
`[]int64`. `main.ts` / `main.tora.ts` use `number`, which is f64 — the
only numeric type TS has. The sum here is 4.9999995e13, below 2^53, so
every language prints the same digits; but the **work** differs, and a
cross-language row on this case therefore carries a semantics column.

The comparator with matching semantics is **bun** (also f64). Rust and Go
are the *i64* reference, i.e. the hardware ceiling for this shape rather
than a peer implementation of the same program.

## Result on M4 Pro (2026-08-30, bench-harness `--runs 5`)

```
torajs (AOT)    20.8 ms   ← 1.98× faster than bun (same semantics)
bun-aot         41.1 ms
rust (i64)      12.1 ms   ← different program: Vec<i64>, not f64
torajs-run      57.3 ms
```

And the same program written with torajs's explicit `i64` annotation —
`let xs: i64[]`, `let sum: i64` — which makes it semantically the *rust*
program:

```
torajs (i64)    11.3 ms   ← faster than rust's 12.1
```

**So there is no abstraction tax on this shape.** The distance between
20.8 and 12.1 is the f64 column, not lost efficiency: `sum = sum + xs[j]`
is a 3-cycle `FADD` on the loop-carried chain plus an `SCVTF`, where the
i64 form is a 1-cycle `add`.

Why the accumulator is f64 is written up in
`.claude/tasks/2026-08-30/perf-w5-accumulator-decomposition.md`. Short
version: `number` is f64 unless every value reaching the slot is provably
integral AND provably below 2^53, and nothing in the pipeline currently
knows what an array holds — a value loaded from `xs` has no interval
fact, so the accumulator that consumes it has none either.

An earlier revision of this file recorded 11.7 ms and "one i64 load + one
i64 add per iteration". That was true when it was written and is not true
now: the read loop is f64 today. The change was bought by correctness (an
out-of-bounds read has to be able to answer `undefined`, and an i64 slot
has no bit pattern for it — ES §10.4.2.1), so it is a deliberate trade,
not a regression to chase. What is worth chasing is proving the bound so
the narrow form can come back with the semantics intact.
