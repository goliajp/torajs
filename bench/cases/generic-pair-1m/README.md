# generic-pair-1m

1M-iteration loop allocating a generic `Pair<number, number>` and reading both fields each iteration. Tests M3.4 — `type Pair<A, B> = { fst: A, snd: B }` instantiated to a concrete struct + heap alloc + field load per iteration.

## Workload

```ts
type Pair<A, B> = { fst: A, snd: B };

function loopSum(n: number): number {
  let sum: number = 0;
  for (let i: number = 0; i < n; i = i + 1) {
    let p: Pair<number, number> = { fst: i, snd: i + 1 };
    sum = sum + p.fst + p.snd;
  }
  return sum;
}

console.log(loopSum(1_000_000));   // 1000000000000
```

## Why this case

Validates the M3.4 monomorphization path — `Pair<number, number>` interns one `Type::Obj(StructId)` layout, every let-binding goes through `__torajs_obj_alloc(16)` + two field writes + two field reads + `__torajs_obj_drop`. The interesting question is how aggressively LLVM's escape analysis + SROA collapses the alloc/drop pair when the struct doesn't escape the loop body.

The Rust reference uses `Box<Pair<i64, i64>>` (with `#[inline(never)]` on `loop_sum`) so each iteration genuinely allocates — without `Box`, rustc keeps the struct on the stack and the loop becomes pure arithmetic. The Go reference uses `&Pair[int64, int64]{...}` (heap-escaping pointer literal) for the same reason.

## Result on M4 Pro (n=5, 1M iters)

```
torajs (AOT)    1.47 ms      ← 1.71× faster than rust
rust            2.51 ms
go              3.01 ms      torajs 2.05× faster
bun-jsc        13.35 ms      torajs 9.08× faster
bun-aot        13.45 ms      torajs 9.15× faster
torajs-jit     21.03 ms
node-v8        93.61 ms      torajs 63.7× faster
```

torajs (AOT) leads because the SSA shape (alloc → write fst → write snd → read fst → read snd → free, all within one block, pointer never crosses the loop boundary) gives LLVM exactly what it needs to elide the malloc/free pair entirely via heap-to-stack promotion + SROA. Rust's `Box<T>` carries `noalias` + drop-glue annotations that make rustc more conservative about the same elision; combined with `#[inline(never)]` to keep the comparison fair, the boxed allocation actually fires every iteration on the rust side.
