# generic-id-1m

10M-iteration loop calling a generic identity function `id<T>(x: T): T`. Tests M3 **monomorphization zero-overhead** — after specialization, `id<number>` is just `return x`, and LLVM mem2reg + inlining should reduce the loop body to the same shape as a non-generic sum loop.

## Workload

```ts
function id<T>(x: T): T { return x; }

function loopSum(xs: number[]): number {
  let sum: number = 0;
  for (let i: number = 0; i < xs.length; i = i + 1) {
    sum = sum + id(xs[i]);
  }
  return sum;
}

let xs: number[] = [];
for (let i: number = 0; i < 10_000_000; i = i + 1) xs.push(i);
console.log(loopSum(xs));   // 49999995000000
```

## Why this case

Validates M3 end-to-end: every call to `id(xs[i])` retargets to the monomorphized `id$$_number` (lowered as a tiny alloca-store-load-return), and the AOT inliner is expected to collapse the entire chain to a no-op. If our monomorphization left any residual overhead (extra type tag, extra indirection, etc.), this case would be slower than `array-sum-1m`.

The Rust reference uses `id::<i64>(x)` with `#[inline(never)]` only on `loop_sum` so the inner generic call is allowed to fully inline — same shape that we expect from torajs's mono+mem2reg pipeline.

## Result on M4 Pro (n=5, 10M iters)

```
torajs (AOT)   14.10 ms      ← within ±5% of array-sum-1m's 13.54 ms ⇒ mono is free
rust           14.48 ms      torajs 1.03× faster
go             34.55 ms      torajs 2.45× faster
bun-jsc        49.43 ms      torajs 3.50× faster
bun-aot        50.88 ms      torajs 3.61× faster
torajs-jit     50.67 ms
node-v8       180.47 ms      torajs 12.8× faster
```

torajs (AOT) is **identical-modulo-noise to a non-generic sum loop** (compare with `array-sum-1m` at 13.54 ms). The 0.38 ms edge over Rust is sub-stddev; meaningful read is "monomorphization carries zero residual cost on the hot path."
