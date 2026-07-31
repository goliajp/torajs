// RFC 20260730-undeclared-ident 刀 2 — §6.2.5.5 GetValue on an
// unresolvable Reference raises a catchable runtime ReferenceError
// (not a compile reject). Covers:
//   - bare read `x + 1` in try/catch: instanceof / .name / .message
//   - §13.5.3 typeof unresolvable stays "undefined" (never throws)
//   - an unevaluated read (dead branch) never throws
//   - read as a call argument
try {
  // @ts-ignore
  x + 1;
  console.log("no throw");
} catch (e) {
  console.log(e instanceof ReferenceError);
  console.log((e as Error).name);
  console.log((e as Error).message);
}
// @ts-ignore
console.log(typeof yy);
const cond = false;
if (cond) {
  // @ts-ignore
  zz + 1;
}
try {
  // @ts-ignore
  console.log(ww);
} catch (e) {
  console.log((e as Error).message);
}
console.log("end");
