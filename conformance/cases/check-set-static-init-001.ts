// S133-4 — `new Set([a, b, c])` static array-literal initializer.
// Pre-S133 check rejected with "`new Set(...)` with iterable
// initializer not yet supported (got 1 args)" since the only
// accepted form was the 0-arg ctor; the spec §24.2.1 takes any
// iterable, but the P5 iterator-protocol plumbing isn't yet wired
// into ctor desugar.
//
// Narrow fix: when args[0] is a literal `Expr::Array` of
// non-Spread elements, check accepts and ssa-lower unrolls into
// `set_create + map_set(_, k_tag, k_val, ANY_UNDEF, 0)` per
// element (same shape `s.add(v)` lowers to). General iterable
// initializers (Map.entries, Set.values, custom Symbol.iterator)
// stay deferred behind the P5 follow-up.

// 1. Number-array initializer.
let s = new Set([1, 2, 3]);
console.log("num size", s.size);
console.log("num has 2", s.has(2));
console.log("num has 4", s.has(4));

// 2. Deduplication — repeated elements collapse per spec.
let s2 = new Set([1, 2, 3, 2, 1]);
console.log("dedup", s2.size);

// 3. String-array initializer with explicit generic.
let s3 = new Set<string>(["a", "b", "c"]);
console.log("str size", s3.size);
console.log("str has b", s3.has("b"));

// 4. Empty-array initializer.
let s4 = new Set([]);
console.log("empty", s4.size);

// 5. Single-element initializer.
let s5 = new Set([42]);
console.log("single", s5.size);
console.log("single has", s5.has(42));

// 6. Regression — 0-arg ctor still works as before.
let s6 = new Set<number>();
s6.add(7);
s6.add(8);
console.log("zero-arg", s6.size);

// 7. Mixed expression elements (not pure literals).
let n = 10;
let s7 = new Set([n, n + 1, n + 2]);
console.log("expr", s7.size);
console.log("expr has 11", s7.has(11));
