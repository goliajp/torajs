// RFC 20260716-primitive-wrapper-substrate 刀 14 — StringWrapper
// receiver in `Object.getOwnPropertyDescriptor(wrapper, "length")`
// per ES §22.1.4.1 String Exotic Object. `length` is a data
// descriptor: `{value: [[StringData]].length, writable: false,
// enumerable: false, configurable: false}`. Pre-fix
// `__torajs_anyv_get_property_descriptor` had no
// `Tag::StringWrapper` cascade arm and answered `undefined`.
//
// Runtime path: reflect.rs cascade adds a StringWrapper arm before
// the DynObj fallback that view-throughs the inner Str cell for
// `length` and answers via the shared `build_data_descriptor`.
// Char-index descriptors (`getOwnPropertyDescriptor(new String(
// "a"), "0")`) are a follow-up — their `value` is a ShortStr
// NaN-box immediate that doesn't fit the (tag, value) shape
// `build_data_descriptor` takes; L3b.

// Non-empty wrapper.
console.log(Object.getOwnPropertyDescriptor(new String("abc"), "length"));
// { value: 3, writable: false, enumerable: false, configurable: false }

// Empty wrapper (`new String()`, NULL inner sentinel).
console.log(Object.getOwnPropertyDescriptor(new String(), "length"));
// { value: 0, writable: false, enumerable: false, configurable: false }

// Long content.
console.log(Object.getOwnPropertyDescriptor(new String("hello world"), "length"));
// { value: 11, writable: false, enumerable: false, configurable: false }

// Non-length key on wrapper → undefined (char-index descriptors
// are the follow-up above).
console.log(Object.getOwnPropertyDescriptor(new String("abc"), "foo"));
// undefined

// Sentinel — Number / Boolean wrappers stay undefined for `length`
// (they have no `length` own property per spec §21.1.4 / §20.3.4).
console.log(Object.getOwnPropertyDescriptor(new Number(42), "length"));
// undefined
console.log(Object.getOwnPropertyDescriptor(new Boolean(true), "length"));
// undefined
