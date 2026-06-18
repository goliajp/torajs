// ES §23.1.3.31 - Array.prototype.splice accepts any array-typed
// receiver, including a literal expression. tr previously panicked
// at lower time with "ssa-lower: unsupported member call shape:
// splice" when the receiver wasn't an Ident bound to a Type::Arr.
// The returned removed sub-array is the user-visible result; the
// literal source's in-place mutation is invisible (no binding holds
// it) which matches bun.

console.log([1, 2, 3, 4].splice(1, 2))

// method-chain receiver — the result of `.toSorted()` is also a
// fresh-owned Array that previously fell into the same panic site.
console.log([4, 3, 2, 1].toSorted().splice(0, 2))

// existing local-binding splice path still works (regression check).
const a = [10, 20, 30, 40]
const removed = a.splice(1, 2)
console.log(removed)
console.log(a)

// deleteCount 0 returns an empty fresh array.
console.log([1, 2, 3].splice(1, 0))
