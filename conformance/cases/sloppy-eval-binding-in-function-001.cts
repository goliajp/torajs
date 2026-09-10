// §19.2.1 — `eval` is an ordinary writable global property, so a sloppy
// write to it from inside a function body (a plain function, a
// Function-constructor body, an update expression) lands on the same
// global the top-level write reaches. bun 1.4.2 rejects the sloppy
// assignment outright ("Invalid assignment target"), so the oracle is
// node's output, recorded in the .expected sidecar.
function f() { eval = 42; }
f();
console.log(typeof eval, eval);
var g = new Function(" ", "eval = 'g';");
g();
console.log(eval);
function h() { return eval++; }
console.log(h(), eval);
function k() { eval = function () { return "k"; }; }
k();
console.log((eval as any)());
// §13.3.6.1 — once `eval` is rebound, `eval(x)` is an ordinary call of
// the new value, not a direct eval (test262 call/tco-non-eval-global).
var cnt = 0;
function fr(n: number): any { if (n === 0) { cnt++; return; } return eval(n - 1); }
eval = fr;
fr(3);
console.log(cnt);
