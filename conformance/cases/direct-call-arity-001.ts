// over-arity direct calls — ES §13.3.6.1: every argument evaluates
// in order; the callee binds only its declared formals (§10.2.11)
function f0() { return "zero"; }
console.log(f0(1, 2, 3));
function f1(a: number) { return a + 1; }
console.log(f1(10, 99, 98));
function typed(a: string, b: number) { return a + b; }
console.log(typed("x", 2, "drop", false));
// IIFE over-arity
(function fun(a) { console.log("iife", a); }(5, 6, 7));
// closure binding over-arity
const g = function (a) { return a; };
console.log(g("kept", "dropped"));
const arrow = (x: number) => x * 2;
console.log(arrow(21, 100));
// indirect: fn value passed as a param, called over-arity
function h(cb: (n: number) => number) { return cb(4, 5, 6); }
console.log(h((n) => n + 40));
// side effects of the extra args still evaluate, in order
let order: string[] = [];
function e(tag: string) { order.push(tag); return 0; }
function s(a: number) { order.push("body:" + a); return a; }
s(e("first"), e("second"), e("third"));
console.log(order.join(","));
// zero-param fn with all-extra args
function z() { return "z"; }
console.log(z(e("z1"), e("z2")));
console.log(order.join(","));
