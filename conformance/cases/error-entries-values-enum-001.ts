// Object.entries / Object.values must see the same enumerable-only
// surface Object.keys does.
//
// An Error instance owns `message` and `stack` with [[Enumerable]]:
// false (§20.5.6.1.1), and `keys` already answered 0 — its lowering
// hands the static name list to a runtime filter. But `entries` and
// `values` unfolded the struct layout at compile time, emitting every
// field, so the same instance answered 0 keys and 3 entries. The fix
// routes an Error-family receiver through the runtime own-walk the
// `any` lane always used.

const e = new Error("m");
console.log(Object.keys(e).length, Object.entries(e).length, Object.values(e).length);

// through the any lane — the same walk, so necessarily the same answer
const a: any = new Error("m");
console.log(Object.entries(a).length, Object.values(a).length);

// NativeError subclasses carry the same flag
const t = new TypeError("m");
console.log(Object.entries(t).length, Object.values(t).length);

// a user subclass inherits it through the extends chain
class W extends Error {
  constructor(m: string) {
    super(m);
  }
}
console.log(Object.entries(new W("m")).length);

// a user subclass with its own field: only that field is enumerable
class V extends Error {
  code = 7;
}
const v = new V("m");
console.log(JSON.stringify(Object.entries(v)), JSON.stringify(Object.values(v)));

// a user assignment IS enumerable, unlike the ctor-installed slots
const d: any = new Error("m");
d.mine = 1;
console.log(JSON.stringify(Object.entries(d)));

// classes outside the Error family keep the compile-time unfold.
// (Homogeneous fields: `Object.values` over a mixed-type layout is a
// pre-existing loud reject on every class, unrelated to this face.)
class Plain {
  x = 1;
  y = 2;
}
console.log(JSON.stringify(Object.entries(new Plain())));
console.log(JSON.stringify(Object.values(new Plain())));

// plain objects and arrays must not move
console.log(JSON.stringify(Object.entries({ a: 1, b: 2 })));
console.log(JSON.stringify(Object.values({ a: 1, b: 2 })));
console.log(JSON.stringify(Object.entries([9, 8])));
