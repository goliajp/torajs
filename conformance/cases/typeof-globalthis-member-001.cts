// `typeof globalThis.<name>` — a known builtin answers what the bare
// name answers; any other name is a run-time fact of the global object
// (undefined when nothing put it there), never a static "function".
var f = function B() { return 1; };
console.log(typeof globalThis.B, typeof globalThis.f);
console.log(typeof globalThis.Array, typeof globalThis.Math, typeof globalThis.parseInt);
console.log(typeof globalThis.nope, typeof globalThis.undefined);
function decl() {}
console.log(typeof decl);
