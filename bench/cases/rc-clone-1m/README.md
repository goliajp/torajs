# rc-clone-1m

Hot-loop measurement of `Rc<T>::clone` + drop, mirroring Rust's `Rc::clone` baseline.

(Despite the case name, the loop runs **10M iterations** — 1M completes too fast to escape startup-time noise on M-series macOS. The "1m" tag is the family identifier; treat the iteration count as a tunable inside `main.tora.ts`/`main.rs`.)

## Workload

- Allocate one `Rc<Pair>` where `Pair = { a: number, b: number }`.
- 10,000,000 iterations: clone the Rc into a function arg, function reads `x.a` and returns it (defeats dead-code elimination via `black_box` on the Rust side; the foreign call to `__torajs_rc_clone` defeats it on the torajs side), the arg drops at scope exit, the loop accumulates the field read.

Each iteration is one strong-count increment + one strong-count decrement + one field load. Net allocation = 1 (the originating `u`).

## Why this case

Validates P2.3.b/c — `__torajs_rc_clone` and the inline drop sequence on a single-threaded refcount path. P2.3.d gate: torajs AOT within 1.2× of Rust. Empirically (M4 Pro, hyperfine):

```
torajs (AOT)  ≈ 0.86 × of rust   # i.e. ~14% faster than Rust on this case
```

Cause: torajs's `__torajs_rc_clone` is an out-of-line foreign call so LLVM can't inline+balance the inc/dec pair across the loop. Rust inlines `Rc::clone` and `black_box` re-introduces the cost, but the inlined version still has slightly more setup per iteration. Both beat 1.2× by a wide margin.

## Languages

- **torajs** — `Rc.new(p); u.clone(); …` via the P2.3.b intrinsics
- **rust** — `Rc::new(p); Rc::clone(&u); …` reference, `black_box` on the field read to defeat constant folding

Bun/node/Go/Python don't have explicit refcount primitives; this case ships without those rows (the bench harness reports them as `skip: no source file`).
