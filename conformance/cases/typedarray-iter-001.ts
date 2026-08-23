// §23.2.3 the iteration family on %TypedArray%.prototype — forEach,
// map, filter, every, some, the four find*, and the two reduce*.
//
// Three things separate these from their Array.prototype twins and
// each shows up below.
//
// The first two checks are in the opposite order: §23.2.3.15 step 2
// validates the receiver and only step 4 asks whether the callback
// is callable, so a non-callable on a healthy view still reports the
// callback while the array family has no step 2 to get in front.
//
// The length is read ONCE, from the validated record. A callback is
// free to write to the view afterwards and the walk keeps going to
// the length it was told.
//
// The destination is a typed view, so map's store coerces the
// callback's answer the same way an assignment would — a fractional
// number truncates, a bool becomes 0/1.

const ta: any = new Int8Array([1, 2, 3, 4]);

const seen: any[] = [];
const r: any = ta.forEach((v: any, i: any, o: any) => {
  seen.push(v * 100 + i);
  seen.push(o.length);
});
console.log("forEach", r, seen.join(","));

console.log("map", ta.map((v: any) => v * 2).join(","));
console.log("map-trunc", ta.map((v: any) => v + 0.9).join(","));
console.log("map-bool", ta.map((v: any) => v > 2).join(","));
console.log("filter", ta.filter((v: any) => v % 2 === 0).join(","));
console.log("filter-none", ta.filter(() => false).length);
console.log("filter-all", ta.filter(() => true).join(","));

console.log("every", ta.every((v: any) => v > 0), ta.every((v: any) => v > 2));
console.log("some", ta.some((v: any) => v > 3), ta.some((v: any) => v > 9));

console.log("find", ta.find((v: any) => v > 2), ta.find((v: any) => v > 9));
console.log("findIndex", ta.findIndex((v: any) => v > 2), ta.findIndex((v: any) => v > 9));
console.log("findLast", ta.findLast((v: any) => v < 3), ta.findLast((v: any) => v < 0));
console.log(
  "findLastIndex",
  ta.findLastIndex((v: any) => v < 3),
  ta.findLastIndex((v: any) => v < 0),
);

// the index the find family reports, and the order it walks in
const order: any[] = [];
ta.findLastIndex((v: any, i: any) => {
  order.push(i);
  return false;
});
console.log("findLast-order", order.join(","));

console.log("reduce", ta.reduce((a: any, v: any) => a + v));
console.log("reduce-init", ta.reduce((a: any, v: any) => a + v, 100));
console.log("reduceRight", ta.reduceRight((a: any, v: any) => a + "" + v));
console.log("reduceRight-init", ta.reduceRight((a: any, v: any) => a + "" + v, "z"));
// §23.2.3.23 step 5 — an initialValue is an argc question, so an
// explicit `undefined` counts as one and the empty view is fine.
const empty: any = new Int8Array(0);
console.log("reduce-empty-init", empty.reduce((a: any, v: any) => a + v, undefined));

// the four arguments the reduce callback sees
const racc: any[] = [];
ta.reduce((a: any, v: any, i: any, o: any) => {
  racc.push(i + ":" + v + ":" + o.length);
  return a;
}, 0);
console.log("reduce-args", racc.join(" "));

// A thisArg is bound for the callback shapes that take one.
const host: any = { base: 1000 };
const withThis: any[] = [];
ta.forEach(function (this: any, v: any) {
  withThis.push(this.base + v);
}, host);
console.log("thisArg", withThis.join(","));

// The length is fixed at step 3: writing through the view inside the
// callback changes what later rounds READ, not how many there are.
const mut: any = new Int8Array([1, 2, 3]);
const mseen: any[] = [];
mut.forEach((v: any, i: any) => {
  mseen.push(v);
  if (i === 0) {
    mut[1] = 99;
  }
});
console.log("mutating", mseen.join(","), mut.join(","));

// A callback that throws stops the walk and propagates.
try {
  ta.map((v: any) => {
    if (v === 3) {
      throw new RangeError("stop at " + v);
    }
    return v;
  });
  console.log("unreachable");
} catch (e: any) {
  console.log("threw", e.constructor.name, e.message);
}

// §23.2.3.15 step 4 — a non-callable callback is a TypeError.
try {
  ta.forEach(3);
  console.log("unreachable");
} catch (e: any) {
  console.log("not-callable", e.constructor.name);
}

// BigInt elements travel through the same walk as cells — each read
// mints a fresh one and each round has to release it.
//
// The callback bodies here stay off BigInt ARITHMETIC on purpose:
// `(v: any) => v * 2n` throws "Cannot mix BigInt and other types"
// on any receiver whatsoever (`f(5n)` where `f: any` does it too),
// which is an any-lane gap that predates this family and is
// recorded rather than worked around.
const big: any = new BigInt64Array([1n, 2n, 3n]);
console.log("big-map", big.map((v: any) => v).join(","));
console.log("big-filter", big.filter((v: any) => v > 1n).join(","));
console.log("big-reduce", big.reduce((a: any, v: any) => a + ":" + v, "s"));
console.log("big-find", big.find((v: any) => v > 1n));
console.log("big-findIndex", big.findIndex((v: any) => v > 1n));
console.log("big-every", big.every((v: any) => v > 0n), big.some((v: any) => v > 2n));
const bseen: any[] = [];
big.forEach((v: any, i: any) => {
  bseen.push(i + ":" + v);
});
console.log("big-forEach", bseen.join(" "));
