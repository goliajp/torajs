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

Validates M6.2 end-to-end: `xs.map(closure)` lowered to a header-body-after loop that calls a heap-env closure on each element and pushes the result onto a freshly-allocated output array. Per element: load element → load fn_ptr from env+0 → indirect call with env as arg 0 → `__torajs_arr_push` (with possible realloc) onto dst.

Rust uses `Box<dyn Fn>` + `black_box(&dyn Fn)` + `#[inline(never)]` to defeat devirtualization; without those, rustc collapses the entire fn-call indirection. Go uses a non-inlined fn + a closure literal — same shape. Both then `iter().map().collect::<Vec<_>>()` (rust) / `for _, x := range xs { ys = append(ys, add(x)) }` (go) which are heavily-optimized library paths.

## Result on M4 Pro (n=5, 10M iters)

```
torajs (AOT)   31.42 ms      ← parity with rust (was 37.49 before fast-path)
rust           31.56 ms
go             25.71 ms      go 1.22× faster
bun-jsc        62.89 ms      torajs 2.00× faster
bun-aot        63.16 ms      torajs 2.01× faster
torajs-jit     84.64 ms
node-v8       280.85 ms      torajs 8.94× faster
```

**M6.2 fast-path applied (one-shot reserve + push_unchecked):** torajs's `xs.map` lowerer now emits `__torajs_arr_reserve(dst, src.length)` once before the loop and uses `__torajs_arr_push_unchecked` per element. The realloc check + cap-doubling that previously fired every push is gone. The before/after gap on this case was 37.49 → 31.42 ms (~16% faster); rust is now within stddev.

Go still leads slightly. Likely cause: Go's slice header lives by-value on the stack and the compiler bulk-vectorizes the per-element write loop; torajs and rust are both going through a heap-resident `(len, cap, data[])` block.
