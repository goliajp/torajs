// T-09.d (v0.4.0) — Object.freeze + isFrozen identity test.
// Mutation guard (panic on frozen field write) is exercised by
// the test's expected-output check via the runtime stderr — but
// since the conformance runner compares stdout only, the throw
// case is validated separately. This fixture covers the happy
// path: freeze sets the bit, isFrozen reads it back, idempotence,
// and unfrozen objects report false.

type Pair = { a: i64, b: i64 }
let o: Pair = { a: 10, b: 20 }

// Pre-freeze: returns false.
console.log(Object.isFrozen(o))

// Freeze returns the same object.
let r: Pair = Object.freeze(o)
console.log(Object.isFrozen(o))
console.log(Object.isFrozen(r))

// Idempotent: freezing twice still reports true.
Object.freeze(o)
console.log(Object.isFrozen(o))

// Independent objects are not frozen by inheritance.
let q: Pair = { a: 1, b: 2 }
console.log(Object.isFrozen(q))

// Pre-freeze field reads work normally (freeze only blocks writes).
console.log(o.a)
console.log(o.b)
