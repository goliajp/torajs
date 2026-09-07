// §7.1.19 ToPropertyKey takes every value, not a shortlist. Whatever
// the key is, ToPrimitive then ToString spells it, and the property
// under that spelling is what `o[k]` reads.
//
// The key domain here had grown one type at a time — string, symbol,
// any, undefined, then an object (rotation 438), then a function
// (rotation 608) — so what was left was everything nobody had written
// a test for: a boolean, a null, a class instance, a regexp, a
// bigint, an array, a Map.
//
// Not here, and measured rather than assumed: an Array / Map / Set /
// String / RegExp / namespace RECEIVER still refuses these keys. Those
// own an element lane that has to be told from the property lane at
// runtime — handing them the same domain made `[1, 2][true]` answer 2,
// reading the boolean as index 1 where §7.1.19 spells it "true".

const o = {};

o[true] = 1;
o[null] = 2;
o[undefined] = 3;
console.log("prims", o[true], o[null], o[undefined]);
console.log("spelled", o["true"], o["null"], o["undefined"]);

// A bigint spells in decimal, no `n`.
o[10n] = 4;
console.log("bigint", o[10n], o["10"]);

// An array joins with commas; a Map is [object Map]; a regexp is its
// own source text.
const arr = [1, 2];
const re = /ab/g;
const m = new Map();
o[arr] = 5;
o[re] = 6;
o[m] = 7;
console.log("heap", o[arr], o[re], o[m]);
console.log("heap spelled", o["1,2"], o["/ab/g"], o["[object Map]"]);

// A class instance goes through its own toString when it has one, and
// through Object.prototype's when it does not.
class Named {
  toString() { return "N"; }
}
class Plain {
  v = 1;
}
o[new Named()] = 8;
o[new Plain()] = 9;
console.log("class", o["N"], o["[object Object]"]);

// A miss still answers undefined rather than throwing.
console.log("miss", ({})[true]);
