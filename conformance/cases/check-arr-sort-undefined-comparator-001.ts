// ES §23.1.3.{29,31} step 1 — `Array.prototype.sort` and
// `toSorted`: "If comparefn is not undefined and not callable,
// throw a TypeError". `undefined` is therefore explicitly
// permitted and routes to the default lexicographic compare.
// Pre-fix tora's strict comparator-type check rejected the
// 1-arg `.sort(undefined)` / `.toSorted(undefined)` form with
// "argument 0: expected Function([T,T], number), got Undefined".

// In-place sort.
const a = [3, 1, 2];
a.sort(undefined);
console.log(a);                      // [1, 2, 3]

const b = [3, 1, 2];
b.sort();
console.log(b);                      // [1, 2, 3]  (sanity — no-arg still works)

// Immutable sibling.
console.log([3, 1, 2].toSorted(undefined));   // [1, 2, 3]
console.log([3, 1, 2].toSorted());            // [1, 2, 3]

// String elements — default cmp via locale_compare.
const s = ["b", "c", "a"];
s.sort(undefined);
console.log(s);                      // ["a","b","c"]

// Numeric default is *lexicographic* on string-converted values
// (spec §23.1.3.29 step 5b) — verify undefined routes the same path.
console.log([10, 2, 30, 4].toSorted(undefined));  // [10, 2, 30, 4]

// Explicit in-place sort comparator still works (sanity — the
// immutable `toSorted(comparator)` SEGV is a separate L3b carry
// (call_fn_value path on arr_slice clone), out of scope).
const cmp = [3, 1, 2];
cmp.sort((a, b) => a - b);
console.log(cmp);                    // [1, 2, 3]

// Source array unchanged after toSorted(undefined).
const src = [9, 5, 7];
const cloned = src.toSorted(undefined);
console.log(cloned);   // [5, 7, 9]
console.log(src);      // [9, 5, 7]
