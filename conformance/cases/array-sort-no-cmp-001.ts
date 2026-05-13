// V3-18 wedge — Array.prototype.sort / toSorted with no
// comparator argument. Per JS spec §22.1.3.27, the default
// comparator converts to string and compares lexicographically;
// the subset uses an element-type-aware `prev > cur`
// predicate inline (Sgt for Number/I64, Ogt for F64,
// __torajs_str_locale_compare for String/Substr) since real
// ToString-then-lex would cost an alloc per pair. Pre-fix
// tora's strict 1-arg signature rejected the no-arg form
// `arr.sort()` with 'expected 1 argument(s), got 0'.
//
// Implementation:
// * check.rs special-cases sort/toSorted on Array<T> when args
//   is empty, returning Array<T> directly (bypasses the
//   strict-arity signature).
// * ssa_lower's sort/toSorted insertion-sort body branches on
//   whether a user comparator was passed: with → call it and
//   test ret > 0; without → element-type-aware direct
//   comparison.

let xs = [3, 1, 4, 1, 5, 9, 2, 6]
xs.sort()
console.log(xs)                        // [ 1, 1, 2, 3, 4, 5, 6, 9 ]

let strs = ["banana", "apple", "cherry"]
strs.sort()
console.log(strs)                      // [ "apple", "banana", "cherry" ]

// toSorted (immutable variant) with no cmp.
let zs = [30, 10, 20].toSorted()
console.log(zs)                        // [ 10, 20, 30 ]

let ss = ["zoo", "alpha", "mango"].toSorted()
console.log(ss)                        // [ "alpha", "mango", "zoo" ]

// No-cmp sort still chainable (sort returns the same array).
let chained = [5, 2, 8, 1].sort().reverse()
console.log(chained)                   // [ 8, 5, 2, 1 ]

// User comparator still works — the wedge just adds the no-arg
// form, doesn't disturb the existing path.
let with_cmp = [3, 1, 4, 1, 5].sort((a: number, b: number) => b - a)
console.log(with_cmp)                  // [ 5, 4, 3, 1, 1 ]
