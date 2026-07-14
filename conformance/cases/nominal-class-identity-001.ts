// RFC 20260715-nominal-class-identity — TS is structural for
// ASSIGNABILITY but nominal for MEMBER OWNERSHIP. tr used to answer the
// second question structurally too ("which class has my field shape?"),
// so a plain object literal inherited a same-shaped class's accessors
// and methods — a silent-wrong needing neither `any` nor a cast:
//
//   class C { a: number = 1; get b(): number { return 999; } }
//   const plain = { a: 1 };
//   plain.b            // tr said 999; tsc rejects, bun says undefined
//   plain.m()          // tr ran C's method body and said 777
//   plain.b = 5        // tr ran C's setter
//
// A class instance now carries its NAME in its type, and a bare struct
// (object literal, or a `type P = {...}` alias) is an instance of no
// class however closely its shape matches. The steal cases are rejected
// at compile time; the three shapes below are the ones that must keep
// working, since assignability stays structural.

class C {
  a: number = 1;
  get b(): number {
    return 999;
  }
  set b(v: number) {
    this.a = v;
  }
  m(): number {
    return 777;
  }
}

// 1. the class's own instance reaches its accessor + method.
const c = new C();
console.log(c.b, c.m());
c.b = 5;
console.log(c.a);

// 2. a class instance is still STRUCTURALLY assignable to a matching
// object type (nominal identity governs ownership, not assignability).
const structural: { a: number } = new C();
console.log(structural.a);

// 3. a class-annotated param reaches the class's members.
function readVia(k: C): number {
  return k.b + k.m();
}
console.log(readVia(new C()));

// 4. a class instance held in a FIELD keeps its name — the member
// access off it still finds the class's method. (`yield*` lifts its
// delegate iterator into exactly such a field, then calls `.next()`.)
class Holder {
  inner: C = new C();
}
const h = new Holder();
console.log(h.inner.m());

// 5. two classes with the SAME field shape stay distinct — the receiver
// names which one it is, so no first-registered-wins reverse lookup.
class D {
  a: number = 2;
  m(): number {
    return 111;
  }
}
const d = new D();
console.log(d.m(), new C().m());

// 6. an object literal that declares its OWN accessor is untouched by
// all this — its accessor lives in its layout, not in a class.
const lit = {
  a: 1,
  get b(): number {
    return 42;
  },
};
console.log(lit.b);
