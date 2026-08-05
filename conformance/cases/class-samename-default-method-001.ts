// Two classes with a same-named method whose defaults differ.
//
// A `obj.m()` call whose receiver type is not known statically finds
// its callee through a table keyed on the bare method name, and two
// owners whose defaults disagree evict the name from it — padding a
// wrong default would be silently wrong, so an unpadded call and its
// honest arity error was the better trade. But the receiver often IS
// knowable: `desugar_classes` rewrites `new C(..)` into a call to the
// synthesized factory `__new_C`, so a direct `new C().m()` names its
// class right there, and a binding initialized from one carries it on
// the same terms the object-literal gate already trusts — bound
// exactly once program-wide, never reassigned.
//
// Without that, `class A { m(x = 1) {} }` beside
// `class B { m(x = 99) {} }` made `a.m()` fail to compile at all
// ("expected 1 argument(s), got 0") on a program every engine runs.

class A {
  m(x: number = 1): number {
    return x;
  }
}
class B {
  m(x: number = 99): number {
    return x;
  }
}

// direct on the constructed value
console.log(new A().m(), new B().m());
// explicit arguments still win
console.log(new A().m(5), new B().m(5));

// through bindings
const a = new A();
const b = new B();
console.log(a.m(), b.m());
console.log(a.m(2), b.m(3));

// three owners, two defaults shared and one different
class C {
  tag(s: string = "c"): string {
    return s;
  }
}
class D {
  tag(s: string = "c"): string {
    return s + "!";
  }
}
class E {
  tag(s: string = "e"): string {
    return s + "?";
  }
}
const c = new C();
const d = new D();
const e = new E();
console.log(c.tag(), d.tag(), e.tag());
console.log(c.tag("x"), d.tag("x"), e.tag("x"));

// a reassigned binding stays on the shared table — the class it was
// initialized from is no longer proof of what it holds
class F {
  v(n: number = 4): number {
    return n;
  }
}
const f0 = new F();
console.log(f0.v());

// several defaulted parameters, partially supplied
class G {
  at(i: number = 1, j: number = 2, k: number = 3): number {
    return i * 100 + j * 10 + k;
  }
}
class H {
  at(i: number = 7, j: number = 8, k: number = 9): number {
    return i * 100 + j * 10 + k;
  }
}
const g = new G();
const h = new H();
console.log(g.at(), h.at());
console.log(g.at(5), h.at(5));
console.log(g.at(5, 6), h.at(5, 6));
