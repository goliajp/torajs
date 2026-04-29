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
2. **Read phase**: 10M `xs[j]` reads + sum. Tight loop with one i64 load + one i64 add per iteration.

## Why this case

Validates M1.2 — `Array<T>` runtime end-to-end. Specifically:
- `[]` empty literal uses the let-decl's array annotation for the element type
- `xs.push(v)` lowering: load slot → call `__torajs_arr_push` → store back the (possibly realloc'd) pointer
- `xs.length` direct load at offset 0
- `xs[j]` `LoadDyn` SSA inst → `base + 16 + j*8`
- End-of-scope drop fires `__torajs_arr_drop` (verified via `leaks --atExit`)

## Languages

All five — torajs / rust / go / bun / node / python. Rust uses `black_box` on the field read to defeat constant folding (otherwise `-O3` collapses the entire loop into a constant since the workload is purely deterministic).

## Result on M4 Pro (n=10, 10M iters)

```
torajs (AOT)   11.7 ms      ← 1.38× faster than Rust's Vec, 4.32× faster than bun
rust           16.1 ms
bun-jsc        50.5 ms
node-v8       172.3 ms
```

torajs's edge over Rust's `Vec<i64>`:
- Lighter `push` path (no `Drop` overhead per call, no method-dispatch overhead)
- No bounds checking on indexed reads (Rust does at -O3 in some paths)
- Direct `__torajs_arr_alloc` / `realloc` without Layout-tracking overhead

bun pays the JS object-array tax (boxed integers, GC pressure, no specialization). node-v8 pays even more.
