// RFC 20260714-t262-top-clusters 刀 4 — any-held class-instance
// method dispatch. The pre-check name rewrite turned `c.next()` into
// `__cm_C__next(c)` unconditionally, and cm_demote only rescued
// builtin-container receivers — an Any receiver was a guaranteed
// checker reject ("expected Struct, got Any"). Now: Any receivers
// demote back to the member-call shape, route through the runtime
// any-method dispatcher, and REAL class methods resolve by name
// through the class-methods table baked next to class_layouts
// (each `__cm_<C>__<m>` body gets a boxed dual-entry adapter; the
// instance rides the env slot into `__this`).

class C {
  next() {
    return 42;
  }
  add(a: number, b: number) {
    return a + b;
  }
}
const c: any = new C();
console.log(c.next());
console.log(c.add(3, 4));

// inheritance: parent methods resolve up the chain
class P {
  hi() {
    return "p";
  }
}
class Q extends P {
  lo() {
    return "q";
  }
}
const q: any = new Q();
console.log(q.hi(), q.lo());

// (override shadowing is exercised at the resolve level; the
// end-to-end `class R extends P { hi() {...} }` shape is gated by a
// PRE-EXISTING checker face — un-annotated return type on an
// override infers Void ("return type mismatch"), typed tier included
// — recorded in the RFC, independent of this blade.)

// miss semantics stay honest: absent property reads undefined,
// calling one is a catchable TypeError
console.log(c.nosuch ? 1 : 0);
try {
  c.nosuchmethod();
} catch (e) {
  console.log("caught");
}

// typed-tier call unchanged
console.log(new C().next());
console.log("done");
