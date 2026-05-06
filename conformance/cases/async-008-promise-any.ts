// T-17.d (v0.5.0) — Promise.any sync fast-path. Returns the first
// FULFILLED Promise's value (skips rejected). All-rejected →
// rejected with the last seen reason (real spec uses an
// AggregateError; MVP simplifies).

let p1 = Promise.resolve(10)
let p2 = Promise.resolve(20)
let p3 = Promise.resolve(30)

let arr: Promise<number>[] = [p1, p2, p3]
let result: number = await Promise.any(arr)
console.log(result)  // 10 — first fulfilled wins

// All-fulfilled inline.
let inline_result: number = await Promise.any([
  Promise.resolve(50),
  Promise.resolve(60),
])
console.log(inline_result)  // 50
