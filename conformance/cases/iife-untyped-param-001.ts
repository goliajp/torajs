// RC-4 F3 — untyped-param IIFE ABI. An unannotated closure param is
// Type::Any at the ABI; the generalized-indirect call arms
// (emit_closure_callee / emit_fnsig_callee) passed raw operand bits
// into the box-shaped lane, so a primitive argument SIGSEGV'd on the
// callee's deref: `(function(a){ return a })(1)` → exit 139
// (test262 asi S7.9_A5.5_T4 / statements-function S13_A2_T1 /
// arguments-object 10.6-6-4 family). Arm-1 (`let f = fn; f(1)`)
// already boxed — this pins the literal-callee forms.

console.log((function (a) { return a; })(1));
console.log((function named(a) { return a; })(2));
console.log(((a) => a)(3));
console.log((function (a, b) { return b; })(4, 5));
console.log((function (a, b) { return a; })(4, 5));
console.log((function (a) { return a; })("str"));
console.log((function (a) { return a; })({ v: 6 }).v);
console.log((function (a) { return a; })(true));
console.log((function (a) { return a; })(1.5));
console.log(1 + (function (t) { return { a: t }; })(2 + 3).a);
let x = (function __func(arg) { return arg; })(1);
console.log(x);
console.log(typeof __func);
