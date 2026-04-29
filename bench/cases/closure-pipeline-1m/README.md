# closure-pipeline-1m

Tight loop calling a function through a fn-pointer arg. 10M iterations of `sum = sum + f(xs[i])` where `f` is passed in by value.

## Workload

```ts
function add1(x: number): number { return x + 1; }

function reduce(xs: number[], f: (n: number) => number): number {
  let sum: number = 0;
  for (let i: number = 0; i < xs.length; i = i + 1) {
    sum = sum + f(xs[i]);
  }
  return sum;
}

let xs: number[] = [];
for (let i: number = 0; i < 10_000_000; i = i + 1) xs.push(i);
console.log(reduce(xs, add1));  // 50000005000000
```

Two phases:
1. **Build phase**: 10M `xs.push(i)` (same as `array-sum-1m`).
2. **Reduce phase**: 10M iterations of `f(xs[i])` indirect call + add. Stresses the fn-pointer call path (M2 Phase B's `InstKind::CallIndirect`).

## Why this case

Validates M2 Phase B end-to-end — first-class fn pointers in user code.

The Rust reference uses `black_box(add1 as fn(i64) -> i64)` to defeat devirtualization (otherwise LLVM inlines `add1` through the static fn-pointer literal and the indirect-call cost is gone). torajs always emits a real `CallIndirect` (no devirt analysis yet), so the comparison measures actual indirect-call overhead on both sides.

## Result on M4 Pro (n=5, 10M iters)

```
torajs (AOT)   12.98 ms      ← 1.50× faster than rust
rust           19.45 ms
go             36.94 ms      torajs 2.85× faster
bun-aot        46.23 ms      torajs 3.56× faster
bun-jsc        46.70 ms      torajs 3.60× faster
node-v8       173.07 ms      torajs 13.3× faster
```

torajs's edge over Rust on indirect-call hot loops:
- LLVM's `call_indirect ptr_value` emits a single `blr` on ARM with our SSA shape.
- Rust's `fn(i64) -> i64` ABI adds tiny per-call overhead from its standard calling-convention guards.
- Bun pays the JS dynamic-dispatch tax + GC pressure on the hot path.
