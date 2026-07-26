// Calling a function held in a class field. The lane that dispatches
// `recv.name(...)` when the named field holds a callable asked the
// receiver's checked type to spell its shape, which an inline struct
// does and a class instance does not — RFC 20260715-nominal-class-
// identity keeps a class nominal and puts the shape elsewhere. So
// `o.f(3)` on an object literal lowered and `c.f(3)` on a class fell to
// "unsupported member call shape", while `const g = c.f; g(3)` — the
// same value reached through a name — worked all along.

class Simple {
  f: (a: number) => number = (a) => a + 1
}
const s = new Simple()
console.log(s.f(3), new Simple().f(10))

// Zero arguments, and a void return.
class Void {
  greet: () => void = () => {
    console.log("hi")
  }
  answer: () => number = () => 42
}
const v = new Void()
v.greet()
console.log(v.answer())

// Several parameters, and parameters that are not numbers.
class Many {
  join: (a: number, b: string) => string = (a, b) => b + a
  neg: (b: boolean) => boolean = (b) => !b
}
const m = new Many()
console.log(m.join(2, "x"), m.neg(true))

// Assigned by the constructor, capturing a constructor parameter.
class Counted {
  step: (n: number) => number
  constructor(by: number) {
    this.step = (n) => n + by
  }
}
console.log(new Counted(5).step(1), new Counted(100).step(1))

// Reassigned after construction: the call reads the slot, not the seed.
class Swap {
  f: (a: number) => number = (a) => a * 2
}
const sw = new Swap()
console.log(sw.f(4))
sw.f = (a) => a * 100
console.log(sw.f(4))

// A field of a nested class instance, and through a binding.
class Leaf {
  f: (a: number) => number = (a) => a * 3
}
class Holder {
  leaf: Leaf = new Leaf()
}
const h = new Holder()
console.log(h.leaf.f(4))
const leaf = h.leaf
console.log(leaf.f(5))

// The object-literal receiver this lane already served must keep
// working, both untyped and through a named type.
const lit = { f: (a: number) => a + 1 }
console.log(lit.f(3))
type T = { f: (a: number) => number }
const typed: T = { f: (a) => a + 2 }
console.log(typed.f(3))

// A real method on the same class is dispatched statically, not through
// this lane, and must be unaffected by the widened gate.
class Both {
  f: (a: number) => number = (a) => a + 1
  method(a: number): number {
    return a * 10
  }
}
const b = new Both()
console.log(b.f(1), b.method(1))

// Builtin receivers keep their own lanes — the widened gate must not
// pull an array or string method into it.
const arr = [3, 1, 2]
arr.push(4)
console.log(arr.length, arr.indexOf(2), "abc".slice(1))

// Calling in a loop: a fresh receiver each turn, and the result
// accumulates rather than reading a stale slot.
class Adder {
  add: (n: number) => number = (n) => n + 7
}
let total = 0
for (let i = 0; i < 4; i++) {
  total = new Adder().add(total)
}
console.log(total)
