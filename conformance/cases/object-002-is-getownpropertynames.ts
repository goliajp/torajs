// v0.2 #3: Object.is + Object.getOwnPropertyNames.
//
// Object.is is `===` with two corner-case overrides:
//   - Object.is(Number.NaN, Number.NaN) === true  (=== returns false)
//   - Object.is(+0, -0)   === false (=== returns true)
// Lowered per arg SSA type — Type::Number routes through the
// __torajs_object_is_f64 runtime helper for the bit-level ±0 check.
//
// Object.getOwnPropertyNames is an alias of Object.keys at lower
// time (tr has no prototype chain, so own == all).

console.log(Object.is(1, 1));
console.log(Object.is(1, 2));
console.log(Object.is("a", "a"));
console.log(Object.is("a", "b"));
console.log(Object.is(true, true));
console.log(Object.is(true, false));

// Number.NaN corner.
const n = Number.NaN;
console.log(Object.is(n, n));
console.log(Object.is(Number.NaN, Number.NaN));
console.log(n === n);

// ±0 corner.
console.log(Object.is(0, 0));
console.log(Object.is(0, -0));
console.log(Object.is(-0, -0));
console.log(0 === -0);

// Mismatched types fall through to false.
console.log(Object.is(1, "1"));

// getOwnPropertyNames mirrors keys exactly.
type Pt = { x: number, y: number };
const p: Pt = { x: 1, y: 2 };
const ks = Object.keys(p);
const ns = Object.getOwnPropertyNames(p);
console.log(ks.length === ns.length);
console.log(ks[0] === ns[0]);
console.log(ks[1] === ns[1]);
console.log(ns[0]);
console.log(ns[1]);
