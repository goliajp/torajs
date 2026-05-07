// T-17.c (v0.5.0) — Promise.allSettled<number> sync MVP. Result is
// Array<{status: string, value: number}>. Each element's `status`
// is 'fulfilled' / 'rejected'; `value` holds the resolved value
// or rejection reason as i64. spec-strict heterogeneous shape
// (`{status: 'fulfilled', value: T}` vs `{status: 'rejected',
// reason: any}`) deferred — needs union types or Array<Any>.
//
// Bun-parity scope: this fixture covers the fulfilled subset
// where tr's MVP and bun's spec-strict outputs agree
// byte-identically (status='fulfilled' + value=N). Rejection-side
// shape divergence (bun uses `.reason`, tr's MVP uses `.value`)
// covered in commit body smoke; will reconverge once T-15.g.6's
// PromiseId interning lets us flow inner T through the result-
// element shape and ship spec-strict union typing.

let p1 = Promise.resolve(10)
let p2 = Promise.resolve(20)
let p3 = Promise.resolve(30)
let arr: Promise<number>[] = [p1, p2, p3]

type Settled = { status: string, value: number }
let r: Settled[] = await Promise.allSettled(arr)

console.log(r.length)         // 3
console.log(r[0].status)      // fulfilled
console.log(r[0].value)       // 10
console.log(r[1].status)      // fulfilled
console.log(r[1].value)       // 20
console.log(r[2].status)      // fulfilled
console.log(r[2].value)       // 30
