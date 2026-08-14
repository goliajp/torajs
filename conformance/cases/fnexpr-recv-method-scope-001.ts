// A `const` holding a `this`-using function expression promotes its
// receiver when every use of the name is receiver-safe. The census
// that proves "declared exactly once" walked the statement list — and
// `desugar_classes` clones each `this`-using method body into a
// receiver-polymorphic `__cmany_` twin, so a const written ONCE inside
// a method was counted TWICE and the candidate was turned away. The
// same program spelled at top level promoted. What the declined half
// answered was the ENCLOSING method's receiver: arrow semantics handed
// to a function expression, which binds `this` at the call site
// (§10.2.1.2).

class M {
  v = 1;
  direct(): any {
    const gd = function () { return (this as any) === undefined };
    return gd();
  }
  called(): any {
    const gc = function () { return (this as any).v };
    return gc.call({ v: 77 });
  }
  applied(): any {
    const ga = function (n: number) { return (this as any).v + n };
    return ga.apply({ v: 5 }, [3]);
  }
}
const m = new M();
console.log(1, m.direct());
console.log(2, m.called());
console.log(3, m.applied());

// The top-level spelling, which answered correctly all along — pinned
// so the two stay one answer rather than two.
const g0 = function () { return (this as any) === undefined };
console.log(4, g0());

// A method that does read `this` itself keeps reading its own
// receiver: the twin exists either way, and the nested function
// expression is the only thing whose binding moved.
class N {
  v = 9;
  both(): any {
    const gb = function () { return (this as any) === undefined };
    return [(this as any).v, gb()];
  }
}
console.log(5, new N().both());

// An arrow in the same slot keeps the lexical `this` (§8.3.4) — the
// half that must NOT move.
class A {
  v = 4;
  arrow(): any {
    const gr = () => (this as any).v;
    return gr();
  }
}
console.log(6, new A().arrow());

// A STATIC method's body is cloned into an `__smany_` twin the same
// way, so the same census counted the same thing twice there.
//
// Every binding above is spelled with its own name on purpose: the
// census is name-keyed program-wide and deliberately coarse, so six
// `const g`s in six scopes decline as one over-declared name — a
// separate bar, and not what this case is measuring.
class S {
  static s(): any {
    const gs = function () { return (this as any) === undefined };
    return gs();
  }
}
console.log(7, S.s());
