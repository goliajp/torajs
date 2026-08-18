// §20.1.3 — Object.prototype methods explicitly .call'ed on a
// null / undefined literal receiver: ToObject's TypeError is a
// RUNTIME answer (t262 probes it with assert.throws), so the
// prototype-call rewrite must not turn it into a compile-time member
// reject. isPrototypeOf orders §20.1.3.3 step 1 first: a primitive V
// answers false before ToObject(this) can throw.
function cls(e: any): string {
  return e instanceof TypeError ? "TypeError" : "other";
}
try {
  Object.prototype.hasOwnProperty.call(undefined, "foo");
  console.log("no-throw");
} catch (e: any) {
  console.log("hop-undef:", cls(e));
}
try {
  Object.prototype.propertyIsEnumerable.call(null, "x");
  console.log("no-throw");
} catch (e: any) {
  console.log("pie-null:", cls(e));
}
try {
  Object.prototype.toLocaleString.call(undefined);
  console.log("no-throw");
} catch (e: any) {
  console.log("tls-undef:", cls(e));
}
console.log("ipo-null-prim:", Object.prototype.isPrototypeOf.call(null, 5));
console.log("ipo-undef-prim:", Object.prototype.isPrototypeOf.call(undefined, "s"));
try {
  Object.prototype.isPrototypeOf.call(null, {});
  console.log("no-throw");
} catch (e: any) {
  console.log("ipo-null-obj:", cls(e));
}
console.log("hop-obj:", Object.prototype.hasOwnProperty.call({ foo: 1 }, "foo"));
