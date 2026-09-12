// RFC 20260912-global-var-cell knife 4-3 — a `function` declared by eval /
// Script text binds its global-object property WHERE THE TEXT RAN, per
// §9.1.1.4.18 CreateGlobalFunctionBinding: deletable for an eval
// (§19.2.1.3), not for a Script (§16.1.7); over an existing configurable
// property the function attributes replace it, over a non-configurable
// writable one only the value changes.
// oracle: node script goal with `__t262_evalScript = s => vm.runInThisContext(s)`.
function desc(n) {
  var d = Object.getOwnPropertyDescriptor(this, n);
  return d === undefined ? "absent" : [typeof d.value, d.writable, d.enumerable, d.configurable].join(",");
}
console.log("before", "ef" in globalThis, "sf" in globalThis);
(0, eval)("function ef() { return 1; }");
console.log("eval-fn", ef(), desc("ef"), delete globalThis.ef, "ef" in globalThis);
__t262_evalScript("function sf() { return 2; }");
console.log("script-fn", sf(), this.sf(), desc("sf"), delete globalThis.sf, "sf" in globalThis);
Object.defineProperty(this, "cfg", { configurable: true, value: 0 });
Object.defineProperty(this, "ncw", { configurable: false, writable: true, enumerable: true, value: 0 });
Object.preventExtensions(this);
__t262_evalScript("function cfg() { return 3; }");
__t262_evalScript("function ncw() { return 4; }");
console.log("over-configurable", cfg(), this.cfg(), desc("cfg"));
console.log("over-non-configurable", ncw(), this.ncw(), desc("ncw"));
