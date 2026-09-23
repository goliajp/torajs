// A builtin namespace / constructor passed to an unannotated parameter
// lowers as `any` (an implicit-generic instantiation used to spell the
// checker's `Object(ns)` as an annotation lowering could not read).
function nm(o) { return o.name; }
console.log(nm(Array), nm(Map), nm(Promise));
function g(o) { return typeof o; }
console.log(g(Math), g(JSON), g(Reflect));
function same(a, b) { return a === b; }
console.log(same(Array, (globalThis as any).Array), same(JSON, (globalThis as any).JSON));
console.log(Object.prototype.toString.call(Object.create(JSON)));
