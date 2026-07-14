// ES §20.5.3.4 — Error.prototype.toString() = name + ": " + message,
// with empty-name / empty-message special cases. tr previously folded
// every struct-instance toString to "[object Object]"; now an
// Error-derived class instance dispatches to the __torajs_error_to_string
// runtime helper (reading the shared message/name Str fields off the OBJ
// layout prefix), while a plain class with no toString still folds to
// "[object Object]" (ES §19.1.3.6). The helper (not an injected class
// method) keeps "toString" out of method_owners, so a plain
// x.toString() on a primitive / unrelated class is unaffected.
console.log((new Error("boom")).toString());
console.log((new Error("")).toString());
console.log((new TypeError("bad")).toString());
console.log((new RangeError("range")).toString());
class MyErr extends Error {
  constructor(m: string) {
    super(m);
  }
}
console.log((new MyErr("custom")).toString());
class D {
  x = 1;
}
console.log((new D()).toString());
// A primitive toString still routes through its own arm (the helper is
// gated on Error-derived class receivers only).
console.log((255).toString(16));
console.log(true.toString());
