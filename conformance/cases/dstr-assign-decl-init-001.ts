// §13.15.2 — a destructuring assignment in a declaration's INIT
// position: `var y = { p: x } = src`. The pattern assigns first; the
// declared binding receives the RHS itself.
var x = 0;
var y = { p: x } = { p: 42 };
console.log(x, y["p"]);

// array pattern
let a = 0;
const y2 = [a] = [7];
console.log(a, y2[0]);

// keyword property name (the t262 ident-name family)
var d = 0;
var y3 = { default: d } = { default: 9 };
console.log(d, y3["default"]);

// rename + default on the pattern slot
let r = 0;
const y4 = { q: r = 5 } = {};
console.log(r, typeof y4);

// comma-separated multi-decl with a pattern link in the middle
let m = 0;
let before = 1, mid = { k: m } = { k: 3 }, after = 2;
console.log(before, m, mid["k"], after);
