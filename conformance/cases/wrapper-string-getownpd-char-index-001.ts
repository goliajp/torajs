// RFC 20260716-primitive-wrapper-substrate 刀 16 — StringWrapper
// receiver in `Object.getOwnPropertyDescriptor(wrapper, "<idx>")`
// per ES §22.1.4.4 [[GetOwnProperty]] for the char-index face. When
// the key is a canonical numeric index (§7.1.22) less than
// `[[StringData]].length`, the descriptor is a data property:
// `{value: char, writable: false, enumerable: true, configurable:
// false}`. Closes 1 of 2 pass→bug residual cases from the
// rotation-113 sweep (`test/built-ins/Object/getOwnPropertyDescriptor
// /15.2.3.3-3-14.js` — `new String("123")` at `"2"` expects value
// `"3"`; pre-fix answered `undefined`).
//
// Runtime path: reflect.rs cascade's `Tag::StringWrapper` arm — after
// the `length` early-return (刀 14) — parses the key as a canonical
// index and, when in range, delegates to `__torajs_anyv_str_index
// _descriptor` (torajs-meta::str_descriptor) which owns the alloc +
// slot-set sequence for the fresh single-code-unit Str `value`.
//
// The char-index parser is `arr_reflect::canonical_index` (shared
// with Array's §10.4.2 canonical-index arm) — the spec §7.1.22 shape
// is identical.

// In-range char indices — expect data descriptors with `writable:
// false, enumerable: true, configurable: false`.
console.log(Object.getOwnPropertyDescriptor(new String("abc"), "0"));
// { value: "a", writable: false, enumerable: true, configurable: false }
console.log(Object.getOwnPropertyDescriptor(new String("abc"), "1"));
// { value: "b", writable: false, enumerable: true, configurable: false }
console.log(Object.getOwnPropertyDescriptor(new String("abc"), "2"));
// { value: "c", writable: false, enumerable: true, configurable: false }

// Out-of-range — `undefined` per §22.1.4.4 step 5.
console.log(Object.getOwnPropertyDescriptor(new String("abc"), "3"));
// undefined
console.log(Object.getOwnPropertyDescriptor(new String("abc"), "100"));
// undefined

// Empty wrapper (`new String()`, NULL inner sentinel) — every index
// out of range.
console.log(Object.getOwnPropertyDescriptor(new String(), "0"));
// undefined

// Non-canonical numeric keys — leading zero and empty are ordinary
// property keys per §7.1.22 → `undefined` (not the char-index arm).
console.log(Object.getOwnPropertyDescriptor(new String("abc"), "01"));
// undefined
console.log(Object.getOwnPropertyDescriptor(new String("abc"), ""));
// undefined

// The `"length"` key still answers the length descriptor (刀 14).
console.log(Object.getOwnPropertyDescriptor(new String("abc"), "length"));
// { value: 3, writable: false, enumerable: false, configurable: false }

// Exact test262 shape — `test/built-ins/Object/getOwnPropertyDescriptor
// /15.2.3.3-3-14.js`.
const str = new String("123");
const desc = Object.getOwnPropertyDescriptor(str, "2");
console.log(desc?.value);   // "3"
