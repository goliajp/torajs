// T-15.g.3 (v0.5.0) — Promise.then(cb) chaining for the i64→i64
// MVP. The runtime allocates a fresh result Promise per .then call,
// enqueues a dispatcher microtask, and drains on await.

function double(v: number): number {
  return v * 2
}

function addOne(v: number): number {
  return v + 1
}

let p = Promise.resolve(20)
let q = p.then(double).then(addOne)
console.log(await q) // 20 * 2 + 1 = 41
