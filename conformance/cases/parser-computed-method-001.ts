// P0.10 — computed-key method shorthand `{ [expr]() { ... } }`
// per ES spec §13.2.5 ComputedPropertyName + MethodDefinition.
// Pre-fix the parser bailed at `(` after `[<key>]` with
// 'expected `:` after `[<key>]` in object literal'. Test262
// uses these for Symbol.toPrimitive / Symbol.iterator hooks
// (annexB/built-ins/escape/to-primitive-* and similar).
//
// Implementation: extend parser's `parse_object_field` computed-
// property branch to accept `(` after `]` as a method shorthand
// — drop the param list and body with paren / brace balance,
// emit a stub `null` value under the synth field name. tora has
// no Symbol.X dispatch substrate yet (lands with P3/P7), so the
// method body is opaque; the parse just needs to succeed so the
// surrounding obj literal still compiles. Real Symbol.toPrimitive
// dispatch lands with P7 iterator-protocol substrate.

// Single computed-key method shorthand.
let obj = { x: 1, [Symbol.toPrimitive]() { return "stub"; } };
console.log(obj.x)                           // 1

// Computed-key method with explicit return type annotation.
let obj2 = {
  y: 2,
  [Symbol.iterator](): string { return "iter-stub"; }
};
console.log(obj2.y)                          // 2

// Multiple computed-key methods.
let obj3 = {
  a: "a-val",
  [Symbol.toPrimitive]() { return 1; },
  [Symbol.iterator]() { return 2; },
  b: "b-val"
};
console.log(obj3.a)                          // a-val
console.log(obj3.b)                          // b-val

// Computed-key method with multiple params.
let obj4 = {
  z: 3,
  [Symbol.toPrimitive](hint: string) { return hint; }
};
console.log(obj4.z)                          // 3
