// Reflect.getPrototypeOf / preventExtensions / isExtensible — §28.1.{4,8,10}
const o = { a: 1 };
console.log(Reflect.getPrototypeOf(o) === Object.prototype);
const arr = [1, 2];
console.log(Reflect.getPrototypeOf(arr) === Array.prototype);

// strict IsObject gate — primitives throw (unlike the Object flavor)
try {
  Reflect.getPrototypeOf("abc" as any);
} catch (e) {
  console.log("caught", e instanceof TypeError);
}
try {
  Reflect.getPrototypeOf(1 as any);
} catch (e) {
  console.log("caught2", e instanceof TypeError);
}

// isExtensible / preventExtensions round trip; preventExtensions answers boolean
console.log(Reflect.isExtensible(o));
console.log(Reflect.preventExtensions(o));
console.log(Reflect.isExtensible(o));
try {
  Reflect.isExtensible(1 as any);
} catch (e) {
  console.log("caught3", e instanceof TypeError);
}
try {
  Reflect.preventExtensions("x" as any);
} catch (e) {
  console.log("caught4", e instanceof TypeError);
}

// reflection faces
console.log(typeof Reflect.getPrototypeOf, Reflect.getPrototypeOf.length);
console.log(typeof Reflect.preventExtensions, Reflect.preventExtensions.length);
console.log(typeof Reflect.isExtensible, Reflect.isExtensible.length);
