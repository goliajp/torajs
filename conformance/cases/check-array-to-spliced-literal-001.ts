// ES §23.1.3.42 — Array.prototype.toSpliced(start, deleteCount)
// is the immutable variant of `splice`: clone the source, splice
// the clone, return it. Source array is never mutated.
//
// Pre-fix tr only accepted Ident-bound receivers (`xs.toSpliced
// (...)` / `globalArr.toSpliced(...)`); literal-array or method-
// chain receivers fell through to the generic "unsupported member
// call shape: toSpliced" panic. This commit adds the generic
// fallback that lowers an arbitrary receiver expression, clones,
// and drops the source when it was fresh-owned (literal / call
// result).
console.log([1, 2, 3, 4].toSpliced(1, 2));    // [ 1, 4 ]
console.log([10, 20].toSpliced(0, 0));        // [ 10, 20 ] — no-op-delete
console.log([10, 20, 30].toSpliced(1, 1));    // [ 10, 30 ]
console.log([10, 20, 30].toSpliced(0, 3));    // []

// Ident receiver path stays intact (was the only working shape
// pre-fix) — guard against regression.
const xs = [1, 2, 3, 4];
console.log(xs.toSpliced(1, 2));              // [ 1, 4 ]
console.log(xs);                              // [1, 2, 3, 4] — unchanged

// Method-chain receiver (the clone-return path of another
// immutable op, then toSpliced on it)
console.log([4, 3, 2, 1].toSorted().toSpliced(0, 1));  // [ 2, 3, 4 ]
