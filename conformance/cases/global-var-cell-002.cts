// The other direction of RFC 20260912-global-var-cell: a write through
// the global object lands IN the binding, and the binding can change
// its runtime type the way a `var` may (`any` slot).
//
// Oracle is node's real script goal (vm.runInThisContext), pinned in
// `.expected` — bun runs a `.cts` entry as CommonJS.
var v = 1;
globalThis.v = 2;
console.log(v, globalThis.v);
this.v = "s";
console.log(v, typeof v, globalThis.v);
with (globalThis) {
  v = 5;
}
console.log(v, globalThis.v);
Object.defineProperty(globalThis, "v", { value: 7 });
console.log(v, globalThis.v);
function g() { v = v + 1; return v; }
console.log(g(), globalThis.v);
var seen = "";
for (var k in globalThis) {
  if (k === "v") seen = seen + k;
}
console.log(seen);
