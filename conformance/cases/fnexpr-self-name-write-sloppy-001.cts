// §15.5.5 — a sloppy write to a function expression's own self-name is
// ignored (PutValue with S = false on an immutable binding): the RHS
// is evaluated, nothing is stored, the name still answers the function.
var f = function B() {
  B = 1;
  return B;
};
console.log(f() === f);
var g = function C() {
  (() => { C = 2; })();
  return C;
};
console.log(g() === g);
var calls = 0;
var h = function D() {
  D = (calls++, 3);
  return D;
};
console.log(h() === h, calls);
