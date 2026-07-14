// ES §20.5.3.4 — Error.prototype.toString() = name + ": " + message,
// with empty-name / empty-message special cases. tr previously folded
// every struct-instance toString to "[object Object]" (universal-method
// arm), shadowing both the injected Error.prototype.toString AND any
// user-class override; now a class that declares OR inherits an own
// toString dispatches to its __cm_<C>__toString (walking to the
// declaring ancestor for inherited methods), so a plain class with no
// toString still folds to "[object Object]" (ES §19.1.3.6).
console.log((new Error("boom")).toString());
console.log((new Error("")).toString());
console.log((new TypeError("bad")).toString());
console.log((new RangeError("r")).toString());
const e: any = new Error("any-boxed");
console.log(e.toString());
const t: any = new TypeError("t-any");
console.log(t.toString());
class MyErr extends Error {
  constructor(m: string) {
    super(m);
  }
}
console.log((new MyErr("custom")).toString());
class C {
  toString(): string {
    return "C-str";
  }
}
console.log((new C()).toString());
console.log((new C() as any).toString());
class D {
  x = 1;
}
console.log((new D()).toString());
console.log(typeof Error.prototype.toString);
