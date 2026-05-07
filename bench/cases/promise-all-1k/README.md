# promise-all-1k

`await Promise.all(arr)` on a 1000-element array of already-
fulfilled Promises. Sums the resulting number array.

## Workload

```ts
let arr: Promise<number>[] = []
for (let i = 0; i < 1000; i = i + 1) {
  arr.push(Promise.resolve(i))
}
let r: number[] = await Promise.all(arr)
let total = 0
for (let i = 0; i < r.length; i = i + 1) {
  total = total + r[i]
}
console.log(total)  // 499500
```

Stresses the `Promise.all` sync fast-path:
1. 1000 `Promise.resolve(i)` allocations
2. `Promise.all(arr)` walks the input array, pre-checks every
   Promise is FULFILLED, builds the result Array, wraps in a
   fulfilled outer Promise
3. await drain + tora-Array unboxed reads

## Per-language notes

- **bun / node**: V8/JSC `Promise.all` — primary baseline.
- **rust / go / python**: skipped (no clean Promise.all analogue).
