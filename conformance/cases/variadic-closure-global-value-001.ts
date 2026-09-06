// The same binding handed out as a VALUE, and the annotated spelling
// of it.
//
// The wrong canonical spelling (see the sibling `-call-001`) was a
// fact about the value, so it was visible wherever the value's type
// was read, not only at call sites: handing the binding out through a
// fn-typed `return` was refused with "expects Function([Rest(Number)],
// Number), got Function([Array(Number)], Number)" — two spellings of
// one parameter, disagreeing.
//
// Writing the annotation out did not rescue it. The checker then read
// the real rest sentinel off the annotation, but the lowering side
// refused the global slot for any `__rest(`-bearing spelling, so the
// two homes disagreed the other way and the same program died at
// "unknown ident". A variadic annotation parses to the closure repr
// already — the boxed dual entry its calls route through lives in the
// env cell — so it takes the global home on the same terms as every
// other closure binding.

const ga = (...xs: number[]): number => xs.length;

// handed out through a fn-typed return, then called
function mk(): (...a: number[]) => number {
  return ga;
}
const h = mk();
console.log(h(1, 2, 3), h(), mk()(4, 5));

// handed out as any, and through a branch
function mkAny(): any {
  return ga;
}
function mkBranch(flag: boolean): (...a: number[]) => number {
  if (flag) {
    return ga;
  }
  return ga;
}
console.log(mkAny()(1, 2), mkBranch(true)(1), mkBranch(false)(1, 2, 3));

// passed as an argument, stored in a field, stored in an element
function take(f: (...a: number[]) => number): number {
  return f(1, 2, 3);
}
function inField(): any {
  const o: any = { f: ga };
  return o.f(1, 2);
}
function inElement(): any {
  const z: any[] = [ga];
  return z[0](1, 2, 3);
}
console.log(take(ga), inField(), inElement());

// the annotated spelling of the binding itself
const gann: (...a: number[]) => number = (...xs: number[]): number =>
  xs.length;
function callAnn(): number {
  return gann(1, 2, 3);
}
function retAnn(): (...a: number[]) => number {
  return gann;
}
console.log(callAnn(), retAnn()(1, 2), gann(7));

// a non-variadic closure binding through the same boundaries, so a
// regression on the ordinary lane shows up here too
const gp = (x: number): number => x + 1;
function mkPlain(): (a: number) => number {
  return gp;
}
function callPlain(): number {
  return gp(7);
}
console.log(mkPlain()(7), callPlain());
