# promise-chain-1k

A 1000-deep `.then(...)` chain off a single source Promise. The
head resolves, the cascade fires through the entire chain via
the microtask queue.

## Workload

```ts
function add1(v: number): number { return v + 1 }

let p = Promise.resolve(0)
for (let i = 0; i < 1000; i = i + 1) {
  p = p.then(add1)
}
console.log(await p)  // 1000
```

This is a different shape than `promise-then-100k`:
- `promise-then-100k` — fan-out: each iteration's chain is independent
- `promise-chain-1k` — fan-in: every `.then` attaches to the previous result, all 1000 fires interleave on the microtask queue

Tests:
- `__torajs_promise_then_simple` per-link alloc cost
- microtask FIFO ordering correctness across deep cascades
- result-Promise rc release as each link's dispatcher runs

## Per-language notes

- **bun / node**: V8/JSC `.then` chain — primary baseline.
- **rust / go / python**: skipped (no clean per-link Promise/.then mirror).
