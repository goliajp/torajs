// An un-annotated static-method param is implicit `any` — the method
// must stay dispatchable through an any-held class object even when
// inference finds no direct call site (inner ClassExpr static face).
var C = class {
  static #m = "outer class";
  static B = class {
    static fieldAccess(o) {
      return o.#m;
    }
  };
};
console.log(C.B.fieldAccess(C));
let b: any = (C as any).B;
console.log(typeof b.fieldAccess);
try {
  b.methodAccess(b);
} catch (e: any) {
  console.log("caught:", e instanceof TypeError);
}
