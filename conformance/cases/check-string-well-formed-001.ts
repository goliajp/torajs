// ES2024 §22.1.3.10 / §22.1.3.30 — `String.prototype.isWellFormed()` /
// `toWellFormed()`. torajs strings are internally UTF-8 so lone
// surrogates can't be encoded by construction; every reachable Str
// is well-formed. `isWellFormed` returns true; `toWellFormed` is the
// identity.

const s = 'hello world'
console.log(s.isWellFormed())
console.log(s.toWellFormed())

// Empty string — vacuously well-formed.
const e = ''
console.log(e.isWellFormed())
console.log(e.toWellFormed())

// Multi-byte UTF-8 stays well-formed (no lone surrogates representable).
const u = '日本語'
console.log(u.isWellFormed())
console.log(u.toWellFormed())

// Mixed types in multi-arg console.log — exercise the typed walker
// path on the boolean result (sanity).
const f = 'abc'.isWellFormed()
const g = 'abc'.toWellFormed()
console.log('isWellFormed result:', f)
console.log('toWellFormed result:', g)

// Chained: round-trip identity.
const r = 'xyz'.toWellFormed().toUpperCase()
console.log(r)
