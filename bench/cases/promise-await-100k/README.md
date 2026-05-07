# promise-await-100k

100k iterations of `total += await Promise.resolve(i)` with no
`.then` attached. Isolates the await + Promise alloc/drop cost
from the `.then` dispatcher — pairs with `promise-then-100k`
for delta analysis.

## Workload

```ts
let total = 0
for (let i = 0; i < 100000; i = i + 1) {
  total = total + (await Promise.resolve(i))
}
console.log(total)  // 4999950000
```

Each iteration:
1. `Promise.resolve(i)` — heap-alloc fulfilled Promise
2. `await` — drain (empty microtask queue → no-op) then read value
3. Promise drop — refcount hits zero, free

Isolates the bare Promise lifecycle + await overhead from .then
dispatch (covered separately by `promise-then-100k`).

## Per-language notes

- **bun / node**: same V8/JSC machinery — primary parity baseline.
- **rust / go / python**: skipped (no clean per-iteration mirror
  of Promise.resolve/await; harness auto-skips missing sources).
