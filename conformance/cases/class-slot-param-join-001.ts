// An override may declare more parameters than the base does; a call
// that omits one binds `undefined` (section 10.2.11). The rows of one
// vtable slot therefore have to agree on a parameter list, and the
// honest agreement is the join: as wide as the widest row, `any` where
// the rows spell different types. Before that join the program was
// refused outright ("one slot, one shape").
class A {
  f(x: number) {
    return x
  }
}
class B extends A {
  f(x: number, y: number) {
    return x + y
  }
}
const xs: A[] = [new A(), new B()]
for (const x of xs) console.log(x.f(2))

// the narrow row is still callable directly, where nothing pads for it
console.log(new A().f(7))

// unannotated rows join the same way, and the supplied argument still
// arrives as itself
class C {
  g(x) {
    return x
  }
}
class D extends C {
  g(x, y) {
    console.log("D", typeof y)
    return x
  }
}
const cs: C[] = [new C(), new D()]
for (const c of cs) console.log(c.g("hi"))

// the wide row may be the base and the narrow one the override
class P {
  h(x, y) {
    return [x, y].join(",")
  }
}
class Q extends P {
  h(x) {
    return "q" + x
  }
}
const ps: P[] = [new P(), new Q()]
for (const p of ps) console.log(p.h(1, 2))
