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
torajs (AOT)   37.49 ms
rust           27.40 ms      torajs 1.37× slower
go             21.45 ms      torajs 1.75× slower
bun-jsc        60.47 ms      torajs 1.61× faster
bun-aot        60.06 ms      torajs 1.60× faster
torajs-jit     81.98 ms
node-v8       250.31 ms      torajs 6.68× faster
```

torajs comfortably beats every JS engine but trails rust + go in this case. The gap is in the inner loop:
- rust's `iter().map(...).collect()` pre-allocates the destination Vec exactly once and bulk-writes — no per-element realloc check.
- go's `append` doubles capacity in-place; with `make([]int64, 0, n)` it would be even closer to a one-alloc loop.
- torajs's `__torajs_arr_push` does a per-element capacity check + amortized realloc; combined with the closure call (load fn_ptr → indirect call → arg 0 = env), the inner loop is meaningfully heavier.

Closing the gap requires a smarter Array runtime (a `reserve(n)` intrinsic the lowerer calls before the loop, OR a fast-path `push_unchecked` after a one-time bound check). Both are tractable; deferred to a follow-up since the M6.2 priority was "iter methods work" not "iter methods sub-rust".
