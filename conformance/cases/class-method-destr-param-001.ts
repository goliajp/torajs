// cluster #1 blade 1 — a class method's destructuring parameter
// synthesizes an unannotated `__param_destr_N` holder; the
// `__this`-first closure-shape arm now defaults it (and every other
// unannotated method param) to `any` instead of rejecting with
// "requires a type annotation" (TS noImplicitAny=false semantics).
class C {
  method([x]) {
    return x;
  }
  pair([a, b]) {
    return a + b;
  }
  obj({ k }) {
    return k;
  }
  plain(v) {
    return v * 2;
  }
}
const c = new C();
console.log(c.method([42]));
console.log(c.pair([1, 2]));
console.log(c.obj({ k: "hi" }));
console.log(c.plain(21));

// class expression — same shape through the __cm___ClassExpr_0__ path
const D = class {
  method([x, y]) {
    return x + y;
  }
};
console.log(new D().method([10, 20]));

// a plain fn that uses `this` gets promoted to __this-first by
// bind_this_param; its untyped params ride the same any default
function usesThis(v) {
  return this !== null && v > 0;
}
console.log(usesThis(5));
