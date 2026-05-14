// P-PARSE.4 — object literal getter / setter shorthand
// `{ get NAME() {...}, set NAME(v) {...} }` per ES spec §12.7.6.
// Pre-fix tora's parser saw `get` / `set` as a regular field
// name and bailed at the following NAME ident with 'expected
// `:` after field name `get`'. Test262's
// language/expressions/array/spread-obj-* and similar suites
// use these pervasively (~30+ cases hit this).
//
// Implementation: parser detects `get NAME(` / `set NAME(`
// at the start of an object-literal field, consumes the param
// list + optional return ann + body braces (brace-balanced),
// and DROPS the body. The synthesised field name encodes the
// kind:
//   `get x() { ... }`   →  `__getter_x: null`
//   `set x(v) { ... }`  →  `__setter_x: null`
//
// Why drop the body: getter / setter bodies typically use
// `this` to refer to the owning object, but tora's `this`
// resolution only exists inside class methods (desugar enforces
// it at check time). Keeping the body as an Expr::ArrowFn would
// route through closure-lift and hit 'bare `this` reached
// check.rs'. Real accessor-descriptor substrate (lazy
// invocation, this-bound to the object) lands in P3
// (property-bag objects) / P7 (class spec) — at that point
// the body becomes live and the synthetic `__getter_*` field
// name flips back to a real accessor descriptor.
//
// Net effect: parse acceptance for getter / setter shorthand.
// Test262 cases that assert "the syntax is accepted" start
// passing; cases that depend on the accessor semantic
// (`o.x` invokes the getter) remain blocked until P3 / P7.

let obj = {
  a: 1,
  b: 2,
  get x() { return this.a + this.b },
  set x(v) { this.a = v },
  c: 3
}
console.log(obj.a)                           // 1
console.log(obj.b)                           // 2
console.log(obj.c)                           // 3

// Just a getter — the surrounding obj still constructs.
let g = { get y() { return 0 } }
console.log(typeof g)                        // object

// Just a setter.
let s = { set z(v) { } }
console.log(typeof s)                        // object

// Multiple getters interleaved with regular fields.
let interleaved = {
  first: 10,
  get bar() { return 99 },
  middle: 20,
  set bar(v) { },
  last: 30,
}
console.log(interleaved.first)               // 10
console.log(interleaved.middle)              // 20
console.log(interleaved.last)                // 30

// Reserved-word property name still works as a regular field.
let kw = { default: 42, type: "number" }
console.log(kw.default)                      // 42
console.log(kw.type)                         // number
