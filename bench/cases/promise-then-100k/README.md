# promise-then-100k

100k iterations of `total += await Promise.resolve(i).then(add1)`.
Stresses the Promise allocator + `.then` dispatcher + microtask
queue + await-drain at full per-iteration cost.

## Workload

```ts
function add1(v: number): number { return v + 1 }

let total = 0
for (let i = 0; i < 100000; i = i + 1) {
  total = total + (await Promise.resolve(i).then(add1))
}
console.log(total)  // 5000050000
```

Each iteration:
1. `Promise.resolve(i)` — heap-alloc a fulfilled Promise wrapping i
2. `.then(add1)` — alloc a result Promise, attach a callback, enqueue a microtask
3. `await` — drain pending microtasks (firing the .then dispatcher,
   which calls add1, resolves the result Promise) then read the
   resolved value

## What this benchmark does NOT measure

This is a **sync-resolve** workload — no I/O, no real suspension,
every microtask runs to completion before the next iteration starts.
The cost being measured is per-iteration substrate overhead, not
scheduler / event-loop performance under contention. Real-suspension
async/await (T-16 state-machine lowering) gets its own bench when
that path lands.

## Per-language notes

- **bun / node**: standard V8/JSC microtask queue, used as the
  primary parity baseline.
- **rust / go / python**: omitted intentionally — their async
  abstractions (futures + executor / channels + goroutines /
  asyncio) don't cleanly mirror the per-iteration Promise/then/
  await shape. The harness auto-skips missing language sources.
