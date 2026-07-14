// RFC 20260714-objlit-accessor blade 3 — CopyDataProperties reaches a
// source accessor through [[Get]] (ES §7.3.25).
//
// Spread and object-rest copy each own key by CALLING the getter, and
// what lands in the target is a DATA property holding the result. The
// spread unfold walked the source's struct layout and loaded each slot
// raw, so a `__getter_b` slot copied the getter CLOSURE across instead
// of invoking it — `rest` came out holding a function under a mangled
// name rather than the value under `b`.
//
// This is the shape test262's `obj-ptrn-rest-getter` family is built on:
//   for (var {...x} of [{ get v() { count++; return 2; } }])
//     -> x.v === 2 and count === 1

// rest copies the getter's RESULT, and calls it exactly once
let count = 0;
const src = { get v() { count = count + 1; return 2; } };
const { ...x } = src;
console.log(x.v, count);

// the getter is not re-run when the copy is read again
console.log(x.v, count);

// a destructured key is omitted from the rest, and its own read still
// goes through the getter
let hits = 0;
const two = {
  get a() { hits = hits + 1; return 10; },
  get b() { hits = hits + 1; return 20; },
};
const { a, ...restTwo } = two;
console.log(a, restTwo.b, hits);

// accessors mixed with plain data fields
const mixed = { n: 1, s: "x", get dbl() { return this.n * 2; } };
const { ...copy } = mixed;
console.log(copy.n, copy.s, copy.dbl);

// spread (not rest) copies through [[Get]] the same way
let spreadHits = 0;
const gsrc = { get g() { spreadHits = spreadHits + 1; return 5; } };
const spread = { ...gsrc, extra: 9 };
console.log(spread.g, spread.extra, spreadHits);

// an inline member wins over a spread-copied accessor on key collision
const over = { ...gsrc, g: 99 };
console.log(over.g);

// the copy is a plain data property — writing it does not go through a
// setter, and does not disturb the source
const w = { get k() { return 1; } };
const { ...wc } = w;
console.log(wc.k, w.k);
