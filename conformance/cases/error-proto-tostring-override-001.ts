// §20.5.3.4 — Error.prototype.toString monkey-patch: instance
// dispatch (typed + any tier, subclass chain) honors the overwritten
// prototype entry; the builtin renders when untouched.
const e0 = new Error("m0");
console.log("builtin:", e0.toString());
Error.prototype.toString = Object.prototype.toString;
console.log("badge:", e0.toString());
console.log("badge-new:", new Error("m1").toString());
const anyRecv: any = new Error("m2");
console.log("badge-any:", anyRecv.toString());
console.log("badge-sub:", new TypeError("m3").toString());
Error.prototype.toString = function (): string {
  return "patched";
};
console.log("closure:", e0.toString());
console.log("closure-any:", anyRecv.toString());
