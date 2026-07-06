// RC-4 — a closure with a value return the static sniff can't type
// (any-param method call / any-on-any arith) used to fall back to a
// VOID return type: the callee dropped its return value and every
// call site read 0 (silent-wrong across the whole untyped-callback
// surface). The fallback is now `any`, mirroring the param default.

// any-param method call flows back out
const g = function (m) { return m.toUpperCase(); };
console.log(g("abc"));

// any-on-any arith: number and string paths
const h = function (a, b) { return a + b; };
console.log(h(1, 2));
console.log(h("x", "y"));

// IIFE shape
console.log((function (m) { return m.toLowerCase(); })("HI"));

// a statically-typeable return keeps its precise inference
const f = function (s: string) { return s.toUpperCase(); };
console.log(f("ok"));

// a body without a value return stays void
["a", "b"].forEach(function (x) { console.log(x); });

// chained: any-ret closure result feeds a second call
const wrap = function (v) { return v + "!"; };
console.log(wrap(g("z")));
