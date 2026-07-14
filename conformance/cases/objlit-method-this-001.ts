// RFC 20260714-objlit-accessor blade 1 — an object-literal method binds
// `this` to the receiver.
//
// The parser desugars `{ m() {} }` to an `Expr::ArrowFn`, which erases
// the one semantic that separates the two forms: a method binds `this`
// to the receiver, an arrow takes the LEXICAL `this`. Meanwhile
// `desugar_classes_pass2` rewrites every `Expr::This` in the arena to
// `Ident("__this")` — on the assumption, stated in its own comment, that
// `this` only ever appears inside a class method. Nothing bound it at an
// object literal, so the body's `this` became a free variable and the
// checker rejected the whole program: "closure `__closure_0` references
// unknown identifier `__this`".
//
// The method now rides a synthetic nominal alias (`__ObjLit_<n>`), the
// same shape a class gets: a TypeDecl the checker's pre-pass resolves
// into `aliases[..]`, and a `__this` param annotated with that name.
// Unlike a class method it keeps its `__env`, so it can still capture.

const o = { a: 5, m() { return this.a * 2; } };
console.log(o.m());

// captures — the one thing a class method cannot do, and the shape
// test262's accessor cases lean on (`{ get v() { count++; ... } }`)
function mkCounter() {
  let c = 0;
  return {
    step() {
      c = c + 1;
      return c;
    },
  };
}
const k = mkCounter();
console.log(k.step(), k.step(), k.step());

// `this` alongside declared params
const p = {
  a: 3,
  add(n: number) {
    return this.a + n;
  },
};
console.log(p.add(4), p.add(-3));

// a method reaching a sibling method through `this` — this is why the
// methods stay in the receiver's own layout
const r = {
  a: 2,
  dbl() {
    return this.a * 2;
  },
  quad() {
    return this.dbl() * 2;
  },
};
console.log(r.dbl(), r.quad());

// an arrow FIELD is not a method: it takes the lexical `this` and must
// keep the plain closure-slot ABI (no receiver pushed)
const mixed = { f: () => 7, g() { return 8; } };
console.log(mixed.f(), mixed.g());

// methods on a literal that crosses a fn return boundary (the inferred
// `__inlobj(..)` ann carries the `__mth(` field out)
function mkPoint(x: number, y: number) {
  return {
    x,
    y,
    sum() {
      return this.x + this.y;
    },
  };
}
console.log(mkPoint(3, 4).sum());

// string-returning method + a method taking no `this` at all
const s = {
  tag: "hi",
  shout() {
    return this.tag + "!";
  },
  plain() {
    return 42;
  },
};
console.log(s.shout(), s.plain());
