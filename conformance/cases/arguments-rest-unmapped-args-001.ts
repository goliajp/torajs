// A rest-tailed fn on the static-argv face: the over-arity values
// live in the rest array, not in injected positional extras — the
// materialized (unmapped, ES §10.4.4.7) arguments = fixed prefix +
// spread of the rest array. Covers: arguments.length fold, over-
// arity index reads, write isolation both directions (arguments
// write must not reach the param or the rest array), and an
// under-filled call site.
function f(a: number, ...b) {
  console.log(arguments.length);
  console.log(arguments[0], arguments[1], arguments[2]);
  arguments[1] = "mut";
  console.log(b[0]);
}
f(1, "x", true);

let seen = 0;
function g(a: number, ...b: any[]) {
  arguments[0] = 99;
  seen = a;
  console.log(arguments.length, b.length);
}
g(7);
console.log(seen);
