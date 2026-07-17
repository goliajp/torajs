// RFC 20260717-objlit-anylane-recv knife 2 — the `{...} as any` cast
// joins the any-lane predicate (was empty-literal-only in the SSA
// promote, so a non-empty cast literal kept the nominal stamp and a
// this-using method SIGSEGV'd through the dynobj face).

// this-using method through an as-any cast literal
const o = { v: 7, f() { return this.v; }, plain() { return 100; } } as any;
console.log(o.f()); // 7
console.log(o.plain()); // 100

// getter reading this
const p = { n: 3, get double() { return this.n * 2; } } as any;
console.log(p.double); // 6

// setter writing this + get/set pair on one prop
const q = {
  _x: 0,
  set x(nv) {
    this._x = nv * 2;
  },
  get x() {
    return this._x + 1;
  },
} as any;
q.x = 21;
console.log(q._x, q.x); // 42 43

// nested literal inside a cast literal rides the same lane
const outer = { inner: { w: 5, get big() { return this.w * 10; } } } as any;
console.log(outer.inner.big); // 50

// data fields + expando keep working on the promoted face
const d = { a: 1, b: "two" } as any;
d.c = true;
console.log(d.a, d.b, d.c); // 1 two true

// empty-literal cast (the rotation-125 promote) does not regress
const e = {} as any;
e.k = 9;
console.log(e.k); // 9

// method args shift correctly past the prepended receiver
const r = { base: 10, add(a, b) { return this.base + a + b; } } as any;
console.log(r.add(1, 2)); // 13
console.log("done");
