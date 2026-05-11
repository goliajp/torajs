// V3-18 m1.h.54 — `type T = { a: number; b: number }` with `;`
// field separators (the canonical TS form, used in most tutorials
// and codebase styles). Pre-fix tora only accepted `,`, hard-
// rejecting the `;` form with "expected `}` to end type body,
// got Semi".

type Foo = { a: number; b: number; c: number }
let o: Foo = { a: 1, b: 2, c: 3 }
console.log(Object.keys(o))     // [ "a", "b", "c" ]
console.log(Object.values(o))   // [ 1, 2, 3 ]
console.log(o.a, o.b, o.c)      // 1 2 3

// Mixed `,` and `;` separators (TS allows it).
type Bar = { x: string; y: number, z: boolean }
let b: Bar = { x: "hi", y: 5, z: true }
console.log(b.x, b.y, b.z)      // hi 5 true

// Trailing semicolon.
type Baz = { p: number; q: number; }
let baz: Baz = { p: 10, q: 20 }
console.log(baz.p + baz.q)      // 30
