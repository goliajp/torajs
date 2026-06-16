// S133-5 — `new Map([[k, v], [k, v], ...])` static array-literal-
// of-pair-arrays initializer. Mirrors the S133-4 Set narrow:
// pre-S133 the only accepted form was `new Map<K, V>()`; the
// common idiom `new Map([["a", 1], ["b", 2]])` rejected with
// "`new Map(...)` with iterable initializer not yet supported".
//
// Spec §23.1.1.1 takes any iterable of [k, v] pairs; the P5
// iterator-protocol substrate isn't yet wired into ctor desugar,
// so check special-cases the static literal-of-2-elem-arrays
// shape and ssa-lower emits one `map_set(map, k_tag, k_val,
// v_tag, v_val)` per pair (the same encoding `m.set(k, v)`
// already lowers to).

// 1. String→Number map initializer.
let m = new Map([["a", 1], ["b", 2], ["c", 3]]);
console.log("size", m.size);
console.log("get a", m.get("a"));
console.log("get c", m.get("c"));

// 2. Number→String map initializer with explicit generic.
let m2 = new Map<number, string>([[1, "x"], [2, "y"]]);
console.log("m2 size", m2.size);
console.log("m2 get 2", m2.get(2));

// 3. Empty-array initializer.
let m3 = new Map([]);
console.log("empty", m3.size);

// 4. Duplicate-key dedup — last write wins per spec §23.1.1.1.
let m4 = new Map([["k", 1], ["k", 2], ["k", 3]]);
console.log("dedup size", m4.size);
console.log("dedup k", m4.get("k"));

// 5. Regression — 0-arg ctor still works.
let m5 = new Map<string, number>();
m5.set("z", 99);
console.log("zero-arg", m5.size);

// 6. Expression values (not pure literals).
let base = 100;
let m6 = new Map([["x", base], ["y", base + 1]]);
console.log("expr", m6.size);
console.log("expr y", m6.get("y"));
