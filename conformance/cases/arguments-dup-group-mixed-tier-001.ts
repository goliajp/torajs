// rotation 364 — mixed-tier dup-named binding groups admit per
// instance: g1's `f` is length-only (argc tier), g2's `f` reads
// values (argv tier); each tier admits its own seeded instance and
// the cross-tier name collision resolves through the boxed dual
// entry (see dup.rs).
function g1() {
  const f = function () { return arguments.length; };
  return f(1, 2, 3);
}
function g2() {
  const f = function () { return arguments[0] + arguments[1]; };
  return f(10, 20);
}
console.log(g1());
console.log(g2());
// three-instance mix: two argv + one argc under one name
function g3() {
  const p = function () { return arguments[0] * 2; };
  return p(4);
}
function g4() {
  const p = function () { return arguments.length; };
  return p(9, 9);
}
function g5() {
  const p = function () { return arguments[1]; };
  return p(7, 8);
}
console.log(g3(), g4(), g5());
