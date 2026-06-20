// S265 — Object.{getPrototypeOf,isExtensible,isSealed,
// preventExtensions,seal}(obj, ...trailing) per ES
// §20.1.2.{12,13,14,16,18,20}. Trailing args silent-drop per
// ES trailing-arg ignore. tora's check.rs `vec![Type::Any]` sig
// rejected 1+ args; ssa_lower already eval-and-drops args[1..]
// for the 4 anyv_* helpers (preventExtensions/isExtensible/
// seal/isSealed) — getPrototypeOf gets the matching skip(1)
// loop added in S265.

const o = { a: 1 };
console.log(typeof Object.getPrototypeOf(o));
console.log(typeof Object.getPrototypeOf(o, 99));
console.log(typeof Object.getPrototypeOf(o, 99, 88, 77));

console.log(Object.isExtensible(o));
console.log(Object.isExtensible(o, 99));
console.log(Object.isExtensible(o, 99, 88));

console.log(Object.isSealed(o));
console.log(Object.isSealed(o, 99));

const p = { x: 1 };
console.log(Object.preventExtensions(p, 99) === p);
console.log(Object.isExtensible(p, 77));

const q = { y: 1 };
console.log(Object.seal(q, 99, 88) === q);
console.log(Object.isSealed(q, 77));
