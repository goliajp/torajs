// §20.2.3.3 → §20.2.1.1 — `Function.call(thisArg, ...texts)` is
// dynamic function creation with the this argument ignored
// (CreateDynamicFunction never reads it — t262 S15.3_A3 asserts
// this). The desugar peels a side-effect-free thisArg and rides the
// same compile-time inline channel as the direct `Function(...)`
// form; a malformed body answers the creation-time SyntaxError.
const f: any = Function.call(this, "return 42;");
console.log(f());
const g: any = Function.call(null, "a", "b", "return a + b;");
console.log(g(2, 3));
const h: any = Function.call("ignored-this", "return typeof this;");
console.log(h());
try {
  Function.call(this, "'use strict'; eval = 1;");
} catch (e) {
  console.log("threw", (e as any).constructor.name);
}
console.log("done");
