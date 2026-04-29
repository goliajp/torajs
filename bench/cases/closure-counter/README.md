# closure-counter

Tight loop calling a **capturing** arrow fn through its env block. 10M iterations of `sum = sum + add(xs[i])` where `add` is `(x: number) => x + offset` — `offset` is read from the enclosing scope.

## Workload

```ts
function loopSum(xs: number[], offset: number): number {
  let add = (x: number): number => x + offset;  // captures `offset`
  let sum: number = 0;
  for (let i: number = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i]);
  }
  return sum;
}

let xs: number[] = [];
for (let i: number = 0; i < 10_000_000; i = i + 1) xs.push(i);
console.log(loopSum(xs, 2));   // 50000015000000
```

## Why this case

Validates M2 end-to-end — closures with implicit captures via heap env block. Distinct from `closure-pipeline-1m` which exercises bare `FnSig` (no captures, no env). Each call here:

1. loads the fn pointer from `env+0` (8-byte heap read)
2. indirect-calls the loaded pointer with `env_ptr` as the hidden first arg

The Rust reference uses `Box<dyn Fn(i64) -> i64>` + `#[inline(never)]` on `loop_sum` + `black_box` on the `&dyn Fn` to defeat devirtualization. Without those, rustc monomorphizes through the only-known-concrete-type and the indirect cost vanishes — the comparison would no longer be apples-to-apples vs torajs's mandatory env-load + indirect call.

## Result on M4 Pro (n=5, 10M iters)

```
torajs (AOT)   20.83 ms
rust           17.85 ms      torajs 1.17× slower
go             32.34 ms      torajs 1.55× faster
bun-jsc        45.05 ms      torajs 2.16× faster
bun-aot        47.64 ms      torajs 2.29× faster
torajs-jit     57.82 ms
node-v8       164.35 ms      torajs 7.89× faster
```

The 17% gap behind Rust is the cost of torajs's MVP capture lowering (env_ptr also doubles as the env-block pointer; one load from offset 0 + one indirect call). Rust's `&dyn Fn` is a fat pointer (data + vtable); the vtable load is hoistable in a tight loop, which gives it a slight edge LLVM can fold against the loop overhead. Closing the gap is a non-goal for M2 — the env-block layout was chosen for simplicity over the alternative `(fn_ptr, env_ptr)` two-word fat-pointer that would need 16-byte returns / passing.
