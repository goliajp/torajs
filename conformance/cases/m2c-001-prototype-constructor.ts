// V3-18 m2.c — Member access on `<prim>.constructor` and
// `Number.prototype` / `Number.name` / `Number.length` etc per
// JS spec §20.2 / §21.1 etc. Subset stubs:
//   .constructor             → ConstPtrNull (typeof → "function")
//   <Ctor>.prototype         → ConstPtrNull (typeof → "object")
//   <Ctor>.name              → "<Ctor>" string literal
//   <Ctor>.length            → 1 (constructor arity)
//
// Most test262 cases that exercise these probe the typeof shape
// or use them as opaque references; the stub round-trips the
// shape correctly without committing to a real prototype object.

let n = 5
console.log(typeof n.constructor)         // function
console.log(typeof Number.prototype)      // object
console.log(typeof Number.name)           // string
console.log(typeof Number.length)         // number
console.log(typeof Math.PI)               // number

let s = "hello"
console.log(typeof s.constructor)         // function
console.log(typeof String.prototype)      // object
console.log(typeof String.name)           // string

let b = true
console.log(typeof b.constructor)         // function
console.log(typeof Boolean.prototype)     // object

// Symbol.prototype access.
console.log(typeof Symbol.prototype)      // object
console.log(typeof Symbol.iterator)       // symbol (well-known)
