// What an arity hole pins a type parameter to. The pad writes
// undefined at a slot with no default of its own so it can reach the
// default behind it; that undefined is the language's binding, not an
// argument the program passed, so it must not narrow the parameter's
// inferred type. The checker's own trailing-pad lane already answers
// this way — an absent argument binds `any`.
function sv<T>(a: T, b: T, tag: string): void {
  console.log(tag + ":" + (a === b));
}

// `x` is an implicit type parameter. Bound to Undefined by the hole,
// the second call below could not unify (Undefined beside Number) and
// the whole program was rejected with "unknown function `sv`".
function f(x, _ = 0) {
  sv(x, undefined, "same");
  sv(x, 2, "diff");
}
f();

// The hole still carries undefined into the callee, and the slot is
// the wide one, so it reads back as undefined and not as null.
class C { m(x, y = 5) { return "m:" + x + ":" + y + ":" + (x === undefined) } }
console.log(new C().m(), new C().m(1));

// Two holes in front of the default, and an explicit type parameter
// rather than an inferred one.
function g<T>(a: T, b, c = 7): string {
  return "g:" + a + ":" + b + ":" + c;
}
console.log(g(undefined, undefined, 7));

// Widening the hole did not widen the arguments beside it: `p` is
// still pinned by what the program passes, and both instantiations
// reach the same generic.
function h(p, _ = 0) {
  sv(p, p, "self");
}
h();
h("s");
