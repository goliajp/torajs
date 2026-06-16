// RFC `20260613-object-property-descriptors` C5 close-mark — Array
// `length` real descriptor + `Object.{isSealed,isExtensible,seal,
// preventExtensions}` real-bit substrate.
//
// Two spec edges in one fixture:
//
// 1. Array's `length` own-property descriptor — `{value: arr.length,
//    writable: true, enumerable: false, configurable: false}` per
//    spec §10.4.2.4 (Array's `length` is writable but neither
//    enumerable nor configurable; differs from a plain object's
//    user-set property which is fully `true`). Wired in
//    `ssa_lower.rs:16405-16428` (`arr_length_descriptor` intrinsic
//    reads the raw `u64` header len + builds the spec descriptor
//    object).
//
// 2. Object's `isSealed` / `isExtensible` / `seal` /
//    `preventExtensions` — the real header-flag substrate (not the
//    pre-C5 always-false placeholder). `Object.seal` flips both the
//    sealed bit and the extensible bit; `Object.preventExtensions`
//    only flips extensible (sealed stays false because the existing
//    props remain configurable).
//
// Wired in code well before this RFC; close-mark debt only.

// 1. Array `length` descriptor — spec data property shape.
let arr = [1, 2, 3];
let d: any = Object.getOwnPropertyDescriptor(arr, 'length');
console.log("1 desc value:", d.value);
console.log("1 desc writable:", d.writable);
console.log("1 desc enumerable:", d.enumerable);
console.log("1 desc configurable:", d.configurable);

// 2. Fresh object — NOT sealed, IS extensible.
let o: any = { a: 1 };
console.log("2 fresh isSealed:", Object.isSealed(o));
console.log("3 fresh isExtensible:", Object.isExtensible(o));

// 4. After Object.seal — both flips: sealed → true, extensible → false.
Object.seal(o);
console.log("4 after seal isSealed:", Object.isSealed(o));
console.log("4 after seal isExtensible:", Object.isExtensible(o));

// 5. After Object.preventExtensions — extensible → false but
//    isSealed stays false (props remain configurable so the object
//    is NOT spec-sealed).
let o2: any = { x: 1 };
Object.preventExtensions(o2);
console.log("5 after preventExt isExtensible:", Object.isExtensible(o2));
console.log("5 after preventExt isSealed:", Object.isSealed(o2));
