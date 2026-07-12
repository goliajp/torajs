// RFC 20260712-array-generic-receiver chunks 2+3a — ES generic
// Array.prototype read family over plain-object receivers. Two
// routes into the new arraylike arm: the reified-cell call/apply
// short-circuit (NULL name bytes → mid is authoritative) and the
// dynobj own-entry probe resolving a stored reified cell (its
// carried mid re-dispatches with the receiver). The AST-level
// `Array.prototype.m.call` rewrite now SKIPS the read family so
// these route through the runtime's spec semantics (length getter
// observability, HasProperty hole gating, callback callability
// checked after the length read).
//
// Acceptance: byte-equal with bun.

// .call on a plain array-like object
var o: any = { 0: 11, 1: 12, length: 2 };
console.log(Array.prototype.every.call(o, (x: any) => x > 10));
console.log(Array.prototype.some.call(o, (x: any) => x > 11));
console.log(Array.prototype.indexOf.call(o, 12));
console.log(Array.prototype.lastIndexOf.call(o, 11));
console.log(Array.prototype.includes.call(o, 12));
console.log(Array.prototype.join.call(o, "-"));
console.log(Array.prototype.at.call(o, -1));
console.log(Array.prototype.slice.call(o, 0, 1));
console.log(Array.prototype.map.call(o, (x: any) => x * 2));
console.log(Array.prototype.filter.call(o, (x: any) => x > 11));
console.log(Array.prototype.find.call(o, (x: any) => x > 11));
console.log(Array.prototype.findIndex.call(o, (x: any) => x > 11));
console.log(Array.prototype.findLast.call(o, (x: any) => x < 12));
console.log(Array.prototype.findLastIndex.call(o, (x: any) => x < 12));
console.log(Array.prototype.forEach.call(o, (v: any, k: any) => console.log("fe", v, k)));
console.log(Array.prototype.reduce.call(o, (a: any, b: any) => a + b));
console.log(Array.prototype.reduce.call(o, (a: any, b: any) => a + b, 100));
console.log(Array.prototype.reduceRight.call(o, (a: any, b: any) => a - b));

// detached-to-expando form — the stored cell's mid re-dispatches
var idx: any = {};
idx.indexOf = Array.prototype.indexOf;
idx[0] = "a";
idx[1] = "b";
idx.length = 2;
console.log(idx.indexOf("b"));

// length getter observability — runs even when the callback throws
var accessed = false;
var g: any = { 0: 1 };
Object.defineProperty(g, "length", {
  get: function () {
    accessed = true;
    return 1;
  },
  configurable: true,
});
try {
  Array.prototype.every.call(g, null);
} catch (e: any) {
  console.log("threw", e instanceof TypeError);
}
console.log("accessed", accessed);

// holes — indexOf skips absent keys (HasProperty gate), includes
// and the find family Get them as undefined
var holes: any = { 0: "x", 2: "y", length: 3 };
console.log(Array.prototype.indexOf.call(holes, undefined));
console.log(Array.prototype.includes.call(holes, undefined));
console.log(Array.prototype.findIndex.call(holes, (v: any) => v === undefined));

// string length coercion + real-Arr receivers stay intact
var sl: any = { 0: 7, length: "1" };
console.log(Array.prototype.map.call(sl, (x: any) => x + 1));
const real = [1, 2, 3, 4];
console.log(Array.prototype.slice.call(real, 1, 3));
console.log(Array.prototype.reduce.call(real, (s: number, x: number) => s + x));

// empty + no init — spec TypeError
try {
  Array.prototype.reduce.call({ length: 0 } as any, (a: any, b: any) => a + b);
} catch (e: any) {
  console.log("reduce-empty", e instanceof TypeError);
}
