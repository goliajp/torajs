// T-19.l (v0.5.0) — `Promise<T>.then(onFulfilled, onRejected)`
// 2-arg form per ES2015. Spec equivalent of `.then(onFulfilled)
// .catch(onRejected)`. Both cb signatures are `(v: T) => T`.
// ssa_lower desugars at the call site to a then→catch chain;
// runtime keeps the simple/closure dispatchers unchanged but
// then_simple_dispatch_ now correctly forwards REJECTED state
// to the result Promise instead of silently calling cb with
// value=0 (the pre-T-19.l behavior — masked because no fixture
// exercised .then on a rejected source).

function ok(v: number): number { return v + 1 }
function err(e: number): number { return e * 100 }

let p1 = Promise.resolve(5).then(ok, err)
console.log(await p1)        // 6 — onFulfilled fires

let p2 = Promise.reject(7).then(ok, err)
console.log(await p2)        // 700 — onRejected fires (5 * 100)

// String T — same shape via i64-roundtripping cb.
function shout(s: string): string { return s + '!' }
function muffle(s: string): string { return '(' + s + ')' }

let p3 = Promise.resolve('hi').then(shout, muffle)
console.log(await p3)        // hi!

let p4 = Promise.reject('boom').then(shout, muffle)
console.log(await p4)        // (boom)
