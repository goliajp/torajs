// ES §10.2.1.4 [[Call]] step 11 — a body that completes normally
// answers `undefined`. A value-returning fn whose body can fall
// through used to infer a concrete scalar ret (`boolean` off
// `if (c) return true;`), and the lowering's tail close had no
// undefined spelling for a Bool slot — the open block terminated
// `unreachable` and running off the end trapped (no output,
// SIGTRAP). Now: the implicit-generics pass routes such fns to an
// `any` ret, and the fall-through close returns the ANY_UNDEF box.
function pick(a: number) {
  if (a === 1) return true;
}
console.log(pick(10));
console.log(pick(1));

function grade(n: number) {
  if (n > 90) return "A";
  if (n > 60) return "B";
}
console.log(grade(95), grade(70), grade(10));

function through(a: number) {
  if (arguments[0] === 1) return true;
}
console.log(through(10));
