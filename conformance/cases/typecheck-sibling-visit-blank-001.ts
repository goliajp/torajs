// A rejected operand must not take its SIBLINGS' visits with it.
// `Checker::type_of` records into `expr_types` only on Ok, so a `?`
// that propagated the first failure left every later sibling with NO
// entry — and inside a monomorph specialization body (whose
// diagnostics are discarded by design) lowering reads exactly that
// table. `lower_binop_with_ids` keys `=== undefined` (ANY_UNDEF tag)
// off the literal's recorded type, so an unvisited `undefined`
// packed the null tag and the guard answered backwards.
//
// The rejected operand here is mixed BigInt/Number arithmetic, a real
// TypeError per §13.7; it sits in the branch that is never taken, so
// the program runs.

// ternary — the condition fails, both branches went unvisited
function condFails(a: any, b: any, o: any, g: any) {
  return g ? a - b : o.q === undefined;
}
console.log(condFails(1n, 2, { x: 9 }, false));

// ternary — the then-branch fails, the else went unvisited
function thenFails(a: any, b: any, o: any, g: any) {
  return g ? o.q === undefined : a - b;
}
console.log(thenFails(1n, 2, { x: 9 }, true));

// object literal — an earlier field fails, later fields went unvisited
function fieldFails(a: any, b: any, o: any, g: any) {
  const v = { m: g ? 0 : a - b, n: o.q === undefined };
  return v.n;
}
console.log(fieldFails(1n, 2, { x: 9 }, true));

// comma — the left half fails, the right half went unvisited
function leftFails(a: any, b: any, o: any, g: any) {
  const v = ((g ? 0 : a - b), o.q === undefined);
  return v;
}
console.log(leftFails(1n, 2, { x: 9 }, true));

// the mirror spelling was always right: `!==` reads the same entry
function mirror(a: any, b: any, o: any, g: any) {
  return g ? a - b : o.q !== undefined;
}
console.log(mirror(1n, 2, { x: 9 }, false));
