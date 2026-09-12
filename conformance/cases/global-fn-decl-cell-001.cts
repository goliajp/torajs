// §16.1.7 GlobalDeclarationInstantiation step 16 — a script's top-level
// `function` declaration is an own data property of the global object
// (`{writable, enumerable, not configurable}`), present before the first
// statement runs, whose value is THE function object every value route
// answers (RFC 20260912-global-var-cell knife 3). A write through the
// object lands in the same storage the reflective faces read back.
//
// Oracle is node's real script goal (vm.runInThisContext), pinned in
// `.expected` — bun runs a `.cts` entry as CommonJS, where a top-level
// function is module-local.
console.log(typeof brandNew, typeof globalThis.brandNew, "brandNew" in globalThis);
var d = Object.getOwnPropertyDescriptor(globalThis, "brandNew");
console.log(d.value === brandNew, d.writable, d.enumerable, d.configurable);
console.log(globalThis.brandNew === brandNew, this.brandNew === brandNew, [brandNew][0] === globalThis.brandNew);
console.log(globalThis.hasOwnProperty("brandNew"), Object.keys(globalThis).includes("brandNew"), delete globalThis.brandNew, "brandNew" in globalThis);
var saved = brandNew;
this.brandNew = "unlikely";
console.log(typeof this.brandNew, this.brandNew);
this.brandNew = saved;
console.log(globalThis.brandNew === saved, globalThis.brandNew());
function brandNew() { return "bn"; }
function t(a) { return "a=" + a + " this=" + typeof this; }
console.log(globalThis.t(7), globalThis.t === t, globalThis.t.length, t.length);
function id(x) { return x; }
console.log(globalThis.id(5), globalThis.id("s"), globalThis.id === id);
function* gen() { yield 1; }
async function af() { return 1; }
console.log(typeof globalThis.gen, typeof globalThis.af, globalThis.gen().next().value);
function dup() { return 1; }
function dup() { return 2; }
console.log(globalThis.dup(), globalThis.dup === dup);
function re() { return "re"; }
var re;
console.log(typeof re, re(), globalThis.re === re);
let L = 1;
const C = 2;
class K {}
console.log(globalThis.hasOwnProperty("L"), globalThis.hasOwnProperty("C"), globalThis.hasOwnProperty("K"), L + C);
