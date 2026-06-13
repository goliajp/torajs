// RFC C5b-2 — Object.preventExtensions / isExtensible real bit.
// Spec §20.1.2.16 / §20.1.2.13: real cells flip FLAG_NON_EXTENSIBLE
// on the heap header (then read back); primitives short-circuit
// (preventExtensions → return self; isExtensible → false).

const o: any = {};
console.log("plain isExt before", Object.isExtensible(o));
const ret = Object.preventExtensions(o);
console.log("plain isExt after", Object.isExtensible(o));
console.log("plain return identity", ret === o);

const a = [1, 2, 3];
console.log("arr isExt before", Object.isExtensible(a));
Object.preventExtensions(a);
console.log("arr isExt after", Object.isExtensible(a));

console.log("num isExt", Object.isExtensible(42));
console.log("str isExt", Object.isExtensible("hi"));
console.log("bool isExt", Object.isExtensible(true));
console.log("null isExt", Object.isExtensible(null));
console.log("undef isExt", Object.isExtensible(undefined));

// preventExtensions on a primitive returns the primitive (spec
// "If Type(O) is not Object, return O"). Verified via identity:
const n: any = 42;
console.log("num prevent identity", Object.preventExtensions(n) === n);
