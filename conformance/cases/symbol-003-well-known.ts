// T-13.c (v0.4.0) — well-known Symbol singletons. Process-level
// lazy-init: each access returns the SAME Symbol value via the
// cached pointer. console.log shows the conventional description
// `Symbol(Symbol.iterator)` etc.
//
// Iterator-protocol integration with for-of dispatch via
// `[Symbol.iterator]()` method lands with v0.5 (alongside async/await
// and the iterator substrate). T-13.c only ships the Symbol values
// themselves so user code can store + compare them.

console.log(Symbol.iterator)
console.log(Symbol.asyncIterator)
console.log(Symbol.toPrimitive)

// Identity: every access returns the same singleton.
console.log(Symbol.iterator === Symbol.iterator)
console.log(Symbol.asyncIterator === Symbol.asyncIterator)
console.log(Symbol.toPrimitive === Symbol.toPrimitive)

// Distinctness across the three.
console.log(Symbol.iterator === Symbol.asyncIterator)
console.log(Symbol.iterator === Symbol.toPrimitive)
console.log(Symbol.asyncIterator === Symbol.toPrimitive)

// Bind to a local + compare.
let it = Symbol.iterator
console.log(it === Symbol.iterator)

// typeof — should be "symbol" matching the JS spec.
console.log(typeof Symbol.iterator)
