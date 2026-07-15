// RFC 20260716-primitive-wrapper-substrate 刀 17 —
// `Object.getOwnPropertyDescriptor(obj, key)` ToPropertyKey coerces
// the key arg per ES §20.1.2.10 step 1 → §7.1.19 →§7.1.17 ToString.
// Closes the pass→incompat residual `test/built-ins/Object/
// getOwnPropertyDescriptor/15.2.3.3-2-39.js` surfaced in the
// rotation-113 sweep (`Object.getOwnPropertyDescriptor(obj, new
// String("Hello"))` — pre-fix checker rejected with "key must be
// string, got Any").
//
// Checker relaxed the arg-1 sig from Type::String to Type::Any in
// both the meta member arm (2-arg spec) and the S315 trailing-arg
// wedge (>=3-arg). SSA lower's `emit_to_string` picks the right
// coercion by arg's static SSA type — StringWrapper Any routes
// through `any_to_str`'s TAG_STRING_WRAPPER arm (刀 2b, view-through
// inner Str cell); I64 routes through `i64_to_str`; etc. The
// runtime helper still takes a raw Str pointer, so any owned Str
// produced by the coerce is dropped after the helper's borrow read.

// StringWrapper key — the test262 15.2.3.3-2-39 shape.
const obj: any = { Hello: 1, world: 2 };
console.log(Object.getOwnPropertyDescriptor(obj, new String("Hello"))?.value);
// 1
console.log(Object.getOwnPropertyDescriptor(obj, new String("world"))?.value);
// 2

// Missing key — undefined.
console.log(Object.getOwnPropertyDescriptor(obj, new String("missing")));
// undefined

// I64 key — ToString(42) = "42".
const arr: any = { "42": "answer", "0": "zero" };
console.log(Object.getOwnPropertyDescriptor(arr, 42)?.value);
// "answer"
console.log(Object.getOwnPropertyDescriptor(arr, 0)?.value);
// "zero"

// Boolean key — ToString(true) = "true".
const flag: any = { true: "yes", false: "no" };
console.log(Object.getOwnPropertyDescriptor(flag, true)?.value);
// "yes"
console.log(Object.getOwnPropertyDescriptor(flag, false)?.value);
// "no"

// Regression: primitive string key still works (fast path).
console.log(Object.getOwnPropertyDescriptor(obj, "Hello")?.value);
// 1

// S315 trailing-arg wedge (>=3 args) also relaxes: StringWrapper
// key + a side-effect trailing arg.
let sideEffect = 0;
console.log(
  Object.getOwnPropertyDescriptor(obj, new String("Hello"), (sideEffect = 1))
    ?.value,
);
// 1
console.log(sideEffect);
// 1

// Exact test262 15.2.3.3-2-39 shape.
const desc = Object.getOwnPropertyDescriptor({ Hello: 1 }, new String("Hello"));
console.log(desc?.value);
// 1
