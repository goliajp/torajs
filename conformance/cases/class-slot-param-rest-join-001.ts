// A rest parameter is the one thing a parameter join cannot widen
// INTO: where a row's fixed parameters end decides where its rest
// begins. When the rows' fixed arities agree the tail is uniform, so
// every row of the slot gains one — a row that never declared a rest
// carries a `__slotrest` it does not read, which is what makes the
// call site pack the trailing arguments for whichever row it resolved
// to. Before that the whole program was refused ("one slot, one
// shape").
class A {
  f(x) {
    return "A" + x
  }
}
class B extends A {
  f(x, ...r) {
    return "B" + x + r.length
  }
}
const xs: A[] = [new A(), new B()]
for (const x of xs) console.log(x.f(1))
for (const x of xs) console.log(x.f(1, 2, 3))

// the variadic row may be the base and the fixed one the override
class C {
  g(x, ...r) {
    return "C" + x + r.length
  }
}
class D extends C {
  g(x) {
    return "D" + x
  }
}
const cs: C[] = [new C(), new D()]
for (const c of cs) console.log(c.g(1, 2, 3))
for (const c of cs) console.log(c.g(1))

// three rows, the tail declared only in the middle one
class E {
  m(x) {
    return "E"
  }
}
class F extends E {
  m(x, ...r) {
    return "F" + r.length
  }
}
class G extends F {
  m(x) {
    return "G"
  }
}
const es: E[] = [new E(), new F(), new G()]
for (const e of es) console.log(e.m(1, 2))

// no fixed parameters at all
class H {
  k() {
    return "H"
  }
}
class I extends H {
  k(...r) {
    return "I" + r.length
  }
}
const hs: H[] = [new H(), new I()]
for (const h of hs) console.log(h.k())
for (const h of hs) console.log(h.k(1, 2))

// rest tails spelling different element types widen to any[]
class J {
  n(x, ...r: number[]) {
    return x + r.length
  }
}
class K extends J {
  n(x, ...r: string[]) {
    return x + r.join("")
  }
}
const js: J[] = [new J(), new K()]
for (const j of js) console.log(j.n(1, 2 as any, 3 as any))

// the variadic row is still callable directly and through the any lane
console.log(new B().f(1, 2, 3), new B().f(1), new A().f(9))
const b: any = new B()
console.log(b.f(1, 2, 3))
