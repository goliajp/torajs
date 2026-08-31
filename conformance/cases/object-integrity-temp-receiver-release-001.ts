// Rotation 543 — the last lane the leak sweep found. Six functions in
// `ssa_lower_call_object_integrity` read the receiver, and the four
// that ANSWER it give the result its own ref via
// `emit_owned_result_inc`. That makes an owned argument temp's
// original stake unowned the moment the call returns, and none of the
// six released it.
//
// The two `Object.preventExtensions` branches needed different
// releases, which only measurement could tell. The ObjectLit branch
// mints its dynobj directly and that operand is a `Type::Ptr` — Copy,
// which `release_owned_temp` drops out of before emitting anything.
// The release was in place and the 59.65 MB did not move. Its any box
// is the same cell with a type whose drop is tag-aware.
//
// 200k churn, AOT product RSS, 1.51 MB flat baseline:
//   Object.preventExtensions({a: 1})   59.65 MB -> 1.88 MB
//   Object.freeze({a: 1})              14.42 MB -> 1.61 MB
//
// Bound controls were flat before and after: `Object.freeze(o)`
// 1.59 MB, `Object.preventExtensions(o)` 1.74 MB.
const o = { a: 1 };
console.log(Object.freeze(o) === o, Object.isFrozen(o), o.a);
console.log(Object.isFrozen({ a: 1 }), Object.isSealed({ a: 1 }));
console.log(Object.isExtensible({ a: 1 }));

const s = { a: 1 };
console.log(Object.seal(s) === s, Object.isSealed(s), Object.isExtensible(s));

const p = { a: 1 };
console.log(Object.preventExtensions(p) === p, p.a, Object.isExtensible(p));

const q = Object.preventExtensions({ a: 1, b: "x" });
console.log(q.a, q.b, Object.isExtensible(q));

const f = Object.freeze({ a: 1 });
console.log(f.a, Object.isFrozen(f));

const arr = [1, 2];
Object.freeze(arr);
console.log(arr.length, Object.isFrozen(arr));

const d = { a: 1 };
console.log(Object.defineProperties(d, {}) === d, d.a);
console.log(Object.setPrototypeOf(d, null) === d, d.a);

console.log(Object.isFrozen(1), Object.freeze(1));
