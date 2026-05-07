// T-19.k (v0.5.0) — Promise.prototype.catch + Promise.prototype
// .finally per ES2015/ES2018. .catch(cb) is shorthand for
// .then(undefined, cb): cb runs only on REJECTED state, result
// resolves with cb's return value. .finally(cb) runs cb on either
// settled state, propagates source state + value to result
// unchanged (cb's return is ignored, void cb).
//
// Sync MVP — source is always settled by the time the dispatcher
// fires (microtask queue empties before exit). Pending-source +
// real callback fan-in lands with T-16 state-machine async/await.

function recover(reason: number): number { return reason * 100 }
function passthrough(v: number): number { return v }

// .catch fires on rejection
let r1 = Promise.reject(7).catch(recover)
console.log(await r1)        // 700

// .catch passes through on fulfillment (cb NOT called)
let r2 = Promise.resolve(5).catch(recover)
console.log(await r2)        // 5

// .catch with passthrough cb (.catch(e => e) absorbs the
// rejection without changing the value)
let r3 = Promise.reject(42).catch(passthrough)
console.log(await r3)        // 42

// .finally runs on fulfillment + propagates value
function noop(): void {}
let r4 = Promise.resolve(99).finally(noop)
console.log(await r4)        // 99

// .finally runs on rejection — but the result is REJECTED so we
// chain .catch to surface the value through the spec-strict path.
let r5 = Promise.reject(11).finally(noop).catch(passthrough)
console.log(await r5)        // 11
