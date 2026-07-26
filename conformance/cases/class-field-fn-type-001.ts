// A class field can be declared a function type. Every other type
// seeds from the annotation's own `undefined` sentinel; a function type
// was held back on a wrong-typed zero, so the factory died on its own
// synthesized `__this` ("declared ClassRef(\"C\"), init has
// Struct([(\"f\", Number)])") and such a field could not be written at
// all — with an initializer, without one, or assigned by the
// constructor.
//
// Seeding it needed the repr to be picked by the SLOT's type: a fn
// annotation parses as a bare function address (Copy, takes the
// Str-family oddball) but the field it lands in is refcounted, and the
// rc write faulted on the immortal cell's read-only page.

class WithInit {
  f: (a: number) => number = (a) => a + 1
}
const wi = new WithInit()
console.log(typeof wi.f, wi.f === undefined)

class Unset {
  f: (a: number) => number
}
const un = new Unset()
console.log(un.f, typeof un.f, un.f === undefined)

// A fn field must not drag its sibling fields down with it — that was
// the loud failure's blast radius.
class Mixed {
  f: (a: number) => number = (a) => a * 2
  n: number = 5
  s: string = "hi"
  d: Date
}
const mx = new Mixed()
console.log(mx.n, mx.s, typeof mx.f, typeof mx.d)

// Assigned by the constructor rather than a field initializer — the
// seed is built before the constructor runs, so this is the shape that
// proves the seed itself type-checks.
class ByCtor {
  g: (a: number) => number
  constructor() {
    this.g = (a) => a + 10
  }
}
console.log(typeof new ByCtor().g)

// Several arities and return types.
class Shapes {
  zero: () => void
  two: (a: number, b: string) => string
  nested: (a: (n: number) => number) => number
}
const sh = new Shapes()
console.log(typeof sh.zero, typeof sh.two, typeof sh.nested)
console.log(sh.zero === undefined, sh.two === undefined)

// A class field holding a class that itself holds a fn field.
class Inner {
  f: (n: number) => number = (n) => n + 1
}
class Outer {
  i: Inner = new Inner()
}
console.log(typeof new Outer().i.f)

// An array-of-fn field already worked (its own seed is a typed empty
// array) and must keep working.
class Holder {
  fs: ((n: number) => number)[] = []
}
const h = new Holder()
h.fs.push((n) => n + 1)
console.log(h.fs.length, h.fs[0](3))

// A generator whose yield type is a function type: the same seed table
// zeroes its step value, and the wrong-typed zero was caught loudly
// there too ("expected Function([Number], Number), got Number").
function* gen(): Generator<(n: number) => number> {
  yield (n) => n + 1
}
const it = gen()
const step = it.next()
console.log(typeof step.value, step.done)

// Construction in a loop: each fresh receiver seeds the same cell, and
// nothing accumulates on it.
let undef = 0
for (let i = 0; i < 4; i++) {
  const fresh = new Unset()
  if (fresh.f === undefined) undef++
}
console.log(undef)
