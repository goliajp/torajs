// An object-literal field holding a CAST to a fn type takes the typed
// lane. No decider had an arm for a cast at all, so such a field
// always refused the literal a global home — variadic or not — and
// every named-fn read of the binding answered "unknown identifier".
// The written face is what the field spells, exactly as it does when
// the same annotation is written on the binding instead.
const c1 = { f: ((a: number): number => a * 2) as (a: number) => number };
function viaFixed(): number {
  return c1.f(21);
}
console.log(viaFixed(), c1.f(1));

// a rest-tail face over a fixed-arity closure: the face is what the
// field call goes through, and the checker's rest test on it routes
// the call to the boxed dual entry
const c2 = { f: ((a: number): number => a * 2) as (...xs: number[]) => number };
function viaFace(): number {
  return c2.f(21);
}
console.log(viaFace(), c2.f(21, 99));

// a cast over a NAME, and a mixed literal — the fn field no longer
// costs the literal ones their slot either
const ga = (...xs: number[]): number => xs.length;
const c3 = { f: ga as (...xs: number[]) => number, n: 5, s: "z" };
function viaIdent(): string {
  return c3.f(1, 2, 3) + c3.s + c3.n;
}
console.log(viaIdent(), c3.f(), c3.n, c3.s);

// the field read out as a value and passed on, from a named fn
function viaValue(): number {
  const k = c2.f;
  return k(7);
}
function useIt(h: (a: number) => number): number {
  return h(4);
}
console.log(viaValue(), useIt(c1.f));
