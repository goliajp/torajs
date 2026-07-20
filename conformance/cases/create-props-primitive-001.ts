// §20.1.2.2 step 3 / §20.1.2.3.1 step 1 — ToObject over a primitive
// Properties argument: key-less wrapper => spec no-op, except a
// non-empty string (its wrapper owns enumerable index props whose
// values are 1-char strings => ToPropertyDescriptor TypeError) and
// null (ToObject throws). Pre-fix numbers SIGSEGV'd through the raw
// pointer unbox and Symbol cells hit the kernel catch-all TypeError.
const shapes: any[] = [1, "", true, undefined, Symbol()];
for (const p of shapes) {
  const o: any = Object.create(null, p);
  console.log(Object.getPrototypeOf(o) === null);
}
try {
  Object.create(null, null);
  console.log("null-no-throw");
} catch (e: any) {
  console.log("null threw", e instanceof TypeError);
}
const sp: any = "ab";
try {
  Object.create(null, sp);
  console.log("str-no-throw");
} catch (e: any) {
  console.log("str threw", e instanceof TypeError);
}
const lp: any = "abcdefghij";
try {
  Object.create(null, lp);
  console.log("longstr-no-throw");
} catch (e: any) {
  console.log("longstr threw", e instanceof TypeError);
}
const t: any = {};
const dp: any = 7;
Object.defineProperties(t, dp);
console.log("dp-num ok", Object.keys(t).length);
const dps: any = "xy";
try {
  Object.defineProperties(t, dps);
  console.log("dp-str-no-throw");
} catch (e: any) {
  console.log("dp-str threw", e instanceof TypeError);
}
