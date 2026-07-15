// RFC 20260716-primitive-wrapper-substrate 刀 4 — `Object(x)` callable
// coercion (ES §20.1.1.1 + ToObject §7.1.18). Primitives mint a fresh
// wrapper cell (`Object(5) === Object(5) === false`), heap objects
// keep identity (`Object(obj) === obj`), null/undef return a fresh
// empty {}.

// Primitive → fresh wrapper of the matching kind.
console.log(typeof Object(5));               // object
console.log(typeof Object("hi"));            // object
console.log(typeof Object(true));            // object
console.log(Object(5) instanceof Number);    // true
console.log(Object("hi") instanceof String); // true
console.log(Object(true) instanceof Boolean); // true

// Each call mints a distinct heap cell — heap identity disjoint.
console.log(Object(5) === Object(5));        // false

// Nullish → fresh empty object (ES §20.1.1.1 step 1a).
console.log(typeof Object(null));            // object
console.log(typeof Object(undefined));       // object
console.log(typeof Object());                // object

// Heap object → identity per ToObject step 3 (Object() on an object
// hands back the same reference).
const o = { a: 1 };
console.log(Object(o) === o);                // true
const arr = [1, 2, 3];
console.log(Object(arr) === arr);            // true

// Wrapper's methods reach the wrapped primitive (刀 3 view-through).
console.log(Object(5).valueOf());            // 5
console.log(Object("hi").length);            // 2
