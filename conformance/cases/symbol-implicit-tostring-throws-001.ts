// §7.1.17 / §7.1.4 — implicit ToString / ToNumber of a Symbol throws
// TypeError; the explicit lanes (String(sym), sym.toString()) answer
// the SymbolDescriptiveString. Locks the test262 String.prototype
// this-as-symbol abrupt cluster (12 cases regressed when the
// rotation-141 Symbol toString wiring let OrdinaryToPrimitive find a
// toString for Symbol cells).
const s: any = Symbol("d");
console.log(String(s));
console.log(s.toString());
try {
  const f: any = String.prototype.repeat;
  f.call(s);
  console.log("no-throw");
} catch (e: any) {
  console.log("repeat threw", e instanceof TypeError);
}
try {
  const c: any = s + "";
  console.log("concat no-throw", c);
} catch (e: any) {
  console.log("concat threw", e instanceof TypeError);
}
try {
  const c2: any = "x-" + s;
  console.log("strcat no-throw", c2);
} catch (e: any) {
  console.log("strcat threw", e instanceof TypeError);
}
try {
  const n: any = s + 1;
  console.log("add no-throw", n);
} catch (e: any) {
  console.log("add threw", e instanceof TypeError);
}
