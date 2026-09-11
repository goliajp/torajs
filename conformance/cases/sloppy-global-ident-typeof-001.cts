// §13.5.1.1 — `typeof` on a name that resolves to no binding does not
// throw, so it asks the object half of the global environment record
// the same question a bare read does and falls back to "undefined"
// instead of the ReferenceError.
//
// Folding it to the constant at compile time was right while nothing
// could write through the global object; it stops being right the
// moment `this.p = 1` in global code can.
//
// Oracle is node's real script goal (vm.runInThisContext), pinned in
// `.expected` — bun runs a `.cts` entry as CommonJS.
this.p = 7;
console.log(typeof p, typeof nope9);
globalThis.s = "x";
console.log(typeof s);
this.q = function (): number {
  return 1;
};
console.log(typeof q);
