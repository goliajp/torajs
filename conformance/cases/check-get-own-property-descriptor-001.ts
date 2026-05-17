// P3.getOwnPropertyDescriptor — `Object.getOwnPropertyDescriptor(obj, key)`
// must return a data-property descriptor object reflecting the
// bucket's stored (value, writable, configurable, enumerable) per
// spec §19.1.2.10. Pre-fix tora rejected at typecheck as
// "Object.getOwnPropertyDescriptor not supported". Now that
// dcf069f's attribute-flag-tracking landed the flag bits in the
// dynobj bucket, the descriptor readback is a straight bucket walk.
//
// Scope: dynobj-backed Any obj only. Builtin descriptor shapes
// (Array.length etc.) are a P4 follow-up; the bare key-not-found
// → undefined case is also covered here.

let a: any = {};
a.x = 7;
const d1: any = Object.getOwnPropertyDescriptor(a, "x");
console.log(d1.value);          // 7
console.log(d1.writable);       // true  (implicit set defaults)
console.log(d1.enumerable);     // true
console.log(d1.configurable);   // true

// defineProperty with explicit flags — descriptor reads back the
// exact flags written.
let b: any = {};
Object.defineProperty(b, "y", { value: "hello", writable: false, configurable: false, enumerable: true });
const d2: any = Object.getOwnPropertyDescriptor(b, "y");
console.log(d2.value);          // hello
console.log(d2.writable);       // false
console.log(d2.enumerable);     // true
console.log(d2.configurable);   // false

// defineProperty with absent flags — descriptor reflects the
// spec-default-false-on-fresh-insert semantics.
let c: any = {};
Object.defineProperty(c, "z", { value: 42 });
const d3: any = Object.getOwnPropertyDescriptor(c, "z");
console.log(d3.value);          // 42
console.log(d3.writable);       // false
console.log(d3.enumerable);     // false
console.log(d3.configurable);   // false

// Missing key — return undefined.
let e: any = {};
const d4: any = Object.getOwnPropertyDescriptor(e, "missing");
console.log(d4 === undefined);  // true
