// V3-18 wedge — accept TS type args on `new ClassName<T>(args)`.
// Subset doesn't mono-instantiate built-in generics by their type
// arg yet (Set / Map / Promise from the user-syntax form ship with
// later phases). For user-defined generic classes the type arg
// is informational — TS infers ctor type from args anyway.
//
// Pre-fix tora's parser bailed with 'expected expression, got
// RParen' on 'new Set<number>()'. Now: depth-aware skip of the
// type-arg portion lets the parse succeed; class-specific
// downstream paths take over from there.

class Box<T> {
  v: T
  constructor(v: T) { this.v = v }
}

// User-defined generic ctor — type arg is accepted (was already
// working without the type arg via inference; this fixture pins
// the explicit form).
let b = new Box<number>(5)
console.log(b.v)                              // 5

let s = new Box<string>("hello")
console.log(s.v)                              // hello

// Nested generic args.
class Pair<A, B> {
  a: A; b: B
  constructor(a: A, b: B) { this.a = a; this.b = b }
}
let p = new Pair<number, string>(1, "x")
console.log(p.a, p.b)                         // 1 x

// Even with `<>` the no-arg parens-required form still works.
let n = new Box<number>(99)
console.log(n.v)                              // 99
