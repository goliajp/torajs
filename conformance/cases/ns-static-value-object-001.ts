// RFC 20260719-ns-static-value-reify B3c-1 — Object statics read as
// VALUES. Covers the three ownership classes the dispatcher arms
// split on: fresh-cell results (keys/values/entries/fromEntries),
// borrow results the arm must own before answering (assign/freeze/
// setPrototypeOf), and the already-owned prototype read.
//
// Two faces stay OUT of this fixture, both pre-existing boundaries
// rather than anything this chunk introduces:
//   * variadic alias arity — `assign(t, a, b)` is a checker reject
//     (the member table pins min/max at 2 params); direct calls are
//     unaffected. RFC B4 残档.
//   * HOF callback position — `xs.map(Object.keys)` is a checker
//     variance reject (`Function([Any], Array(String))` vs the
//     element-shaped signature `map` demands), the same typed-slot
//     boundary `fn_addr` throws on. RFC B4.
// An `assign` target must be `any`-typed: a struct-typed literal has
// no slot to grow, which the typed tier rejects at compile time and
// the any lane answers as a catchable TypeError.

const src = { a: 1, b: 2 };

// fresh-cell family — each call answers a brand-new array
const keys = Object.keys;
const values = Object.values;
const entries = Object.entries;
console.log(keys(src));
console.log(values(src));
console.log(entries(src));

// a second call must answer an independent array, not a stale one
const k1 = keys(src);
const k2 = keys(src);
console.log(k1, k2, k1 === k2);

// borrow-return family — the static answers its own receiver, so the
// arm has to raise the count before the caller drops it
const assign = Object.assign;
const target: any = { x: 1 };
const out = assign(target, { y: 2 });
console.log(out, out === target);

const freeze = Object.freeze;
const frozen = freeze({ p: 1 });
console.log(frozen);

const isFrozen = Object.isFrozen;
console.log(isFrozen(frozen), isFrozen({ q: 1 }), isFrozen(5));

// already-owned prototype read — the arm must NOT inc a second time
const getProto = Object.getPrototypeOf;
console.log(getProto([]) === Array.prototype);
console.log(getProto({}) === Object.prototype);
console.log(getProto(Object.prototype));

// The receiver is `any`-typed on purpose: `setPrototypeOf` over a
// STRUCT-typed literal silently no-ops (tr answers false where bun
// answers true) on the direct typed-tier call too, so that gap
// predates this chunk and is recorded as its own L3b item.
const setProto = Object.setPrototypeOf;
const base: any = { greet: 1 };
const proto_target: any = { own: 2 };
const child = setProto(proto_target, base);
console.log(getProto(child) === base, child.own);

// fresh dynobj
const fromEntries = Object.fromEntries;
console.log(fromEntries([["a", 1], ["b", 2]]));

// reflection face — name / length / native toString, the same three
// probes the Math family answers through
console.log(keys.name, values.name, assign.name, getProto.name);
console.log(keys.length, assign.length, setProto.length);
console.log(String(keys));
console.log(keys);

// the cell is interned: two reads of the same static are identical
console.log(Object.keys === Object.keys);

// call re-dispatch — receiver-less, thisArg ignored.
// `.apply` is NOT covered: it rejects with `not callable: type Any`
// on every ns-static cell, Math included, so the gap predates this
// chunk (the B1/B2/B3a fixtures only ever exercised `.call`).
// Recorded as its own L3b item rather than papered over here.
console.log(keys.call(null, src));

// churn — a leaked or over-released borrow shows up here
let n = 0;
for (let i = 0; i < 2000; i++) {
  const t: any = { x: 1 };
  assign(t, src);
  n += keys(t).length;
}
console.log(n);
