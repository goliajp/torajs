// V3-18 m1.h.22 — `new Foo` (no parens) is equivalent to
// `new Foo()` per JS spec §13.3.5 NewExpression. Pre-fix the
// parser hard-rejected with "expected `(` after `new Foo`".
//
// test262 uses both forms; the no-parens form is also common
// in idiomatic code: `new Date`, `new Array`, `new Object`.

class Foo {
  x: number
  constructor() { this.x = 42 }
}

let a = new Foo
console.log(a.x)

let b = new Foo()
console.log(b.x)

// Local-scope class — same shape.
class Pair {
  a: number
  b: number
  constructor(a: number = 1, b: number = 2) { this.a = a; this.b = b }
}
let p1 = new Pair
console.log(p1.a, p1.b)

let p2 = new Pair()
console.log(p2.a, p2.b)

let p3 = new Pair(10, 20)
console.log(p3.a, p3.b)
