// §13.3.9 — an optional chain that ends in a call. When the base is
// nullish the WHOLE chain short-circuits to undefined and the
// arguments never evaluate; when it is not, what remains is an
// ordinary member call, and a missing method there is an ordinary
// TypeError rather than another undefined.
//
// tr had no lane for `Call { callee: OptChain }`: it lowered the chain
// to a VALUE and called that. So the receiver was gone before the call
// (a method reading `this` saw nothing), a nullish base reached a call
// on undefined and threw, and some shapes had no lowering at all.

class B {
  v = 7
  g(x) {
    return "g:" + this.v + ":" + x
  }
}

// the receiver survives the chain
const b: B | null = new B()
console.log(b?.g(1))

// a nullish base answers undefined
const nb: B | null = null
console.log(nb?.g(1))

// …and the arguments never run
let n = 0
function arg() {
  n = n + 1
  return 1
}
console.log(nb?.g(arg()), n)

// the base is evaluated exactly once
let made = 0
function mk() {
  made = made + 1
  return new B()
}
console.log(mk()?.g(2), made)

// an `any` receiver, and one laundered so no class name is knowable
const ab: any = new B()
console.log(ab?.g(3))
function id(z: any): any {
  return z
}
console.log(id(new B())?.g(4))

// object literals: typed-nullable, and plain
const o: { v: number; f: () => string } | null = {
  v: 5,
  f() {
    return "o:" + this.v
  },
}
console.log(o?.f())
const p = {
  v: 6,
  f() {
    return "p:" + this.v
  },
}
console.log(p?.f())

// a MISSING method on a non-nullish receiver is a TypeError — the one
// place `?.` does not answer undefined
const any1: any = { z: 1 }
try {
  any1?.nosuch()
  console.log("no throw")
} catch (e) {
  console.log("caught")
}

// a missing FIELD read is still undefined
console.log(any1?.nosuch)

// chained, and through a nullable field
class C {
  b: B | null = new B()
}
const c: C | null = new C()
console.log(c?.b?.g(5))
const c2 = new C()
c2.b = null
console.log(c2.b?.g(6))

// a non-nullish receiver keeps working through a plain call
console.log(new B().g(7))

// arguments evaluate left to right in the hit branch
let order = ""
function t(s) {
  order = order + s
  return s
}
console.log(b?.g(t("a") + t("b")), order)
