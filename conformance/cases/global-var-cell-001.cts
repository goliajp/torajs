// §16.1.7 GlobalDeclarationInstantiation — a script's top-level `var`
// is an own data property of the global object over the SAME storage
// as the binding (RFC 20260912-global-var-cell): `{writable, enumerable,
// not configurable}`, present before the first statement runs, read
// through every reflective face.
//
// Oracle is node's real script goal (vm.runInThisContext), pinned in
// `.expected` — bun runs a `.cts` entry as CommonJS, where a top-level
// `var` is module-local.
console.log(typeof v, globalThis.v, "v" in globalThis);
var v = 1;
function f() { return v; }
console.log(f(), globalThis.v, this.v, typeof globalThis.v);
var d = Object.getOwnPropertyDescriptor(globalThis, "v");
console.log(d.value, d.writable, d.enumerable, d.configurable);
console.log(globalThis.hasOwnProperty("v"), Object.keys(globalThis).includes("v"));
console.log(delete globalThis.v, "v" in globalThis, v);
var u;
console.log(globalThis.hasOwnProperty("u"), globalThis.u, typeof u);
let L = 1;
const C = 2;
console.log(globalThis.hasOwnProperty("L"), globalThis.hasOwnProperty("C"), L + C);
