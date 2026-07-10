// Chunk 809 — `any`-annotated top-level bindings a named fn reads
// promote to a NaN-box global slot. Pre-fix the checker registered
// the literal's type (String) over the explicit `any` annotation
// (rejecting every cross-type write), and the lowerer never promoted
// Any at all (named-fn reads answered unknown ident).

// read through a named fn (const + let)
let a1: any = "s";
function g1() { return a1 }
console.log(g1());
const a2: any = 5;
function g2() { return a2 + 1 }
console.log(g2());

// cross-type write from a named-fn body
let a3: any = "boxed";
function w3() { a3 = 42 }
w3();
console.log(a3);

// same-type write
let a4: any = "s";
function w4() { a4 = "x" }
w4();
console.log(a4);

// typeof follows the runtime tag across reassignment
let a5: any = "s";
function t5() { return typeof a5 }
console.log(t5());
a5 = 42;
console.log(typeof a5);

// null init, fn-body write
let a6: any = null;
function w6() { a6 = "set" }
w6();
console.log(a6);

// heap payloads: member read and element read
let a7: any = { v: 7 };
function g7() { return a7.v }
console.log(g7());
let a8: any = [1, 2];
function g8() { return a8[0] }
console.log(g8());

// repeated reads keep the box alive
let a9: any = "keep";
function g9() { return a9 }
console.log(g9(), g9());

// main-only any binding keeps the local home
let a10: any = "s";
a10 = 42;
console.log(a10);
