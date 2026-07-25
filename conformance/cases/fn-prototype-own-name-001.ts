// A plain function's `prototype` is one of its own properties.
//
// §20.2.4 creates `prototype` {writable: true, enumerable: false,
// configurable: false} together with the function, right after the
// `length` / `name` pair. tr already answered it on a READ — the mint
// is lazy and lands in the closure's expando dict on first access (RFC
// 20260721 刀 9) — but the reflection surfaces did not know about it:
// `getOwnPropertyNames` listed only `["length","name"]`, and
// `getOwnPropertyDescriptor(f, "prototype")` answered undefined until
// something had read `f.prototype` first.
//
// Both now answer from the same §20.2.4 arm, so they agree whether or
// not the lazy mint has happened, and the ORDER is independent of it:
// the own-name list always emits `prototype` with the virtual pair and
// filters the (possibly already-minted) expando entry out of the dict
// walk. Otherwise an expando written before the first `.prototype` read
// would sort ahead of it.
//
// An arrow or method form owns no `prototype` at all (§10.2.5 does not
// run MakeConstructor) — the header flag the compiler stamps is exactly
// that distinction.

const f: any = function g(a: any, b: any) {
  return a;
};
console.log("names", JSON.stringify(Object.getOwnPropertyNames(f)));
// non-enumerable, so the enumerable-only surfaces stay empty
console.log("keys", JSON.stringify(Object.keys(f)));
console.log("json", JSON.stringify(f));

// the descriptor answers before anything has read `.prototype`
const d: any = Object.getOwnPropertyDescriptor(f, "prototype");
console.log("desc", typeof d.value, d.writable, d.enumerable, d.configurable);
// and the value is the same object the read answers
console.log("same", d.value === f.prototype);
console.log("ctor-backref", f.prototype.constructor === f);

// reading `.prototype` first does not change the order
const afterRead: any = function q() {};
const held = afterRead.prototype;
console.log("after-read", JSON.stringify(Object.getOwnPropertyNames(afterRead)));
console.log("after-read-held", typeof held);

// an expando written before the first `.prototype` read must not sort
// ahead of it
const withExpando: any = function r() {};
withExpando.extra = 1;
console.log("expando", JSON.stringify(Object.getOwnPropertyNames(withExpando)));
console.log("expando-keys", JSON.stringify(Object.keys(withExpando)));

// arrow forms own no prototype
const arrow: any = (x: any) => x;
console.log("arrow", JSON.stringify(Object.getOwnPropertyNames(arrow)));
console.log("arrow-value", arrow.prototype);
console.log("arrow-desc", Object.getOwnPropertyDescriptor(arrow, "prototype"));

// `in` / hasOwnProperty agree with the list
console.log("in", "prototype" in f, "prototype" in arrow);
console.log("hasOwn", f.hasOwnProperty("prototype"), arrow.hasOwnProperty("prototype"));
