// 563-06 / 564-03 — a builtin constructor reads as the class it is.
// `console.log(Map)` used to print `[Function: Map]` because a builtin
// ctor value is an interned closure cell, not a registered class
// object. bun asks JSC whether the callee is a class constructor:
// almost every builtin answers yes, the typed arrays name their
// %TypedArray% parent, and Promise — written as a plain function in
// JSC — is the one that keeps the function form. The `extends` face
// of a user class reads the same predicate, which is why
// `class P extends Promise {}` prints with no `extends` at all.
console.log(Object, Array, Number, String, Boolean);
console.log(Symbol, BigInt, RegExp, Date, Function);
console.log(Map, Set, WeakMap, WeakSet, WeakRef, Iterator);
console.log(ArrayBuffer, Uint8Array, Float64Array, BigInt64Array);
console.log(Promise);
console.log(Error, TypeError, RangeError);

// the same cells reached as `.constructor`
console.log(({} as any).constructor, [].constructor, (5).constructor);
console.log(Promise.prototype.constructor);

// nested — the class form carries at every depth
console.log([Number, Map, Uint8Array]);

// the `extends` face: a user class, a builtin class, Promise
class A extends Object {}
class B extends Uint8Array {}
class C extends Error {}
class D extends C {}
class P extends Promise<number> {}
console.log(A, B, C, D, P);
console.log(class extends Object {});
console.log(class extends Uint8Array {});
