# throw-catch-100k

100K iterations of `throw` + `catch` inside a single fn. Tests M4 exception cost.

## Workload

```ts
function trial(i: number): number {
  try {
    throw i;
  } catch (e) {
    return e;
  }
}

let total: number = 0;
for (let i: number = 0; i < 100000; i = i + 1) {
  total = total + trial(i);
}
console.log(total);   // 4999950000
```

## Why this case

Validates the M4 throw path end-to-end at scale. Each `trial` does:

1. `throw i` → `__torajs_throw_set(i)` (write `throw_active=1` + `throw_value=i`) → `br catch_blk` (since try_stack has the catch as innermost handler — no fn-level propagation needed for in-fn try/catch)
2. catch entry → `__torajs_throw_take()` (load value + clear active) → bind to `e`
3. `return e` (normal return path, no throw_check needed)

All within one fn frame. No stack unwinding, no DWARF tables, no setjmp — just a flag write + a branch + a flag read.

## Result on M4 Pro (n=5, 100K iters)

```
torajs (AOT)    1.41 ms      ← 306× faster than rust, 18× faster than bun
torajs-jit      9.70 ms
go              8.56 ms      torajs 6.07× faster
bun-jsc        25.30 ms      torajs 17.9× faster
bun-aot        25.58 ms      torajs 18.1× faster
node-v8       152.92 ms      torajs 108× faster
rust          431.70 ms
```

torajs's throw is essentially free per call (one i64 store + branch + i64 load). Rust's `panic::catch_unwind` walks the DWARF unwind tables every panic — accurate semantics (drop-glue, stack traces) but a heavy fixed cost per invocation. Go's `panic+defer+recover` is markedly faster than Rust because Go's runtime has a streamlined unwind path, but it still involves traversing the goroutine stack. JS engines (bun, node) sit in the middle: their throw is engine-level fast but goes through their internal exception object construction.

**Caveat — semantic gap:** torajs's M4 throw is intentionally minimal. It does NOT yet:
- run drop-glue across stack frames during throw (M4.2 only handles in-fn finally + `emit_drops_for_owned_locals` at fn-exit; mid-frame heap-owned locals on the throw path leak in the current MVP)
- support catching by value-type pattern (catch always binds the raw i64)
- carry stack traces or arbitrary Error-class payloads (only number throws supported)

These are deliberate scope cuts to ship a working try/catch + finally first. They're tracked for follow-up phases. The 1.41ms perf number is honest for the workload exactly as written, but is NOT a claim that torajs offers a 300× faster throw than Rust at full feature parity — it's a 300× speedup at "the subset I implement so far".
