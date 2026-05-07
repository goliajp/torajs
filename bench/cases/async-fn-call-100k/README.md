# async-fn-call-100k

100k calls to a user-declared `async function double(v: number)`,
awaiting each result. Exercises the async-fn desugar path:
`async function f() { return e }` → `function f() { return
Promise.resolve(e) }` at AST time.

## Workload

```ts
async function double(v: number): Promise<number> {
  return v + v
}

let total = 0
for (let i = 0; i < 100000; i = i + 1) {
  total = total + (await double(i))
}
console.log(total)  // 9999900000
```

Per iteration:
1. `double(i)` — async-fn desugars to `Promise.resolve(i + i)` →
   heap-alloc fulfilled Promise wrapping `2i`
2. `await` — drain (empty queue) + read value + drop Promise

Pairs with `promise-await-100k` to isolate the async-fn-call site
overhead from the bare-await overhead. Per spec, the desugared
body returns `Promise.resolve(...)`, so the runtime path overlaps
with bare `await Promise.resolve(...)`.

## Per-language notes

- **bun / node**: V8/JSC async-fn — primary baseline.
- **rust / go / python**: skipped (no clean per-call await mirror).
