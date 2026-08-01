// RFC 20260801-arguments-escape-face knife 1 — IIFE static-argv face:
// bare `arguments` escaping an IIFE as a first-class value.

// 1. return escape, zero-param over-arity
const a = (function () { return arguments; })(1, "x", true);
console.log(a.length, a[0], a[1], a[2]);

// 2. assign escape
let cap: any = null;
(function () { cap = arguments; })("p", "q");
console.log(cap.length, cap[0], cap[1]);

// 3. pass-to-call escape
function takeLen(o: any) { return o.length; }
console.log((function () { return takeLen(arguments); })(7, 8, 9));

// 4. under-arity: declared 3, passed 1 — length is 1
const u = (function (x: any, y: any, z: any) { return arguments; })(42);
console.log(u.length, u[0]);

// 5. mixed declared + extra
const m = (function (first: any) { return arguments; })("a", "b", "c");
console.log(m.length, m[0], m[1], m[2]);

// 6. arguments.length + escape in the same body
let n6 = 0;
let cap6: any = null;
(function () { n6 = arguments.length; cap6 = arguments; })(5, 6);
console.log(n6, cap6.length, cap6[0]);

// 7. for-of over arguments
(function () {
  for (const v of arguments) console.log("v:", v);
})(10, 20, 30);

// 8. index literal beyond declared params
console.log((function () { return arguments[1]; })("i0", "i1"));

// 9. spread expansion stays static
function sum3(p: any, q: any, r: any) { return p + q + r; }
console.log((function () { return sum3(...arguments); })(1, 2, 3));

// 10. out-of-range read is undefined
const oo = (function () { return arguments; })(1);
console.log(oo.length, oo[5]);
