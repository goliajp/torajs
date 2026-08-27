// Where a row's fixed parameters end decides where its rest begins,
// so when the rows of one slot disagree the tail has to begin where
// the VARIADIC row says it does — that row has no other reading of the
// argument list. The other row's extra parameters are then inside the
// tail, and it reads them back out at the head of its own body. This
// whole family used to be `exit 139`, then a loud refusal.
class A {
  f(x, y = 5) {
    return "A" + (x + y)
  }
}
class B extends A {
  f(x, ...r) {
    return "B" + (x + r.length)
  }
}
const xs: A[] = [new A(), new B()]
for (const x of xs) console.log(x.f(1))
for (const x of xs) console.log(x.f(1, 2))
for (const x of xs) console.log(x.f(1, 2, 3))

// more than one swallowed position, and no defaults on them
class C {
  g(x, y, z) {
    return "C" + x + y + z
  }
}
class D extends C {
  g(x, ...r) {
    return "D" + x + r.length
  }
}
const cs: C[] = [new C(), new D()]
for (const c of cs) console.log(c.g(1, 2, 3))
for (const c of cs) console.log(c.g(1))

// the variadic row is the OVERRIDE and takes more fixed parameters
// than the base, so the base pads up to the tail instead
class E {
  m(x) {
    return "E" + x
  }
}
class F extends E {
  m(x, y, ...r) {
    return "F" + x + y + r.length
  }
}
const es: E[] = [new E(), new F()]
for (const e of es) console.log(e.m(1, 2, 3))
for (const e of es) console.log(e.m(1))

// a swallowed position keeps its default, evaluated in the body
class G {
  k(x, y = "d", z = 9) {
    return "G" + x + y + z
  }
}
class H extends G {
  k(x, ...r) {
    return "H" + x + r.length
  }
}
const gs: G[] = [new G(), new H()]
for (const g of gs) console.log(g.k(1))
for (const g of gs) console.log(g.k(1, "z"))
for (const g of gs) console.log(g.k(1, "z", 0))

// a later default reading an earlier swallowed parameter still sees
// the value that parameter's own guard settled
class I {
  n(x, y = 2, z = y + 1) {
    return "I" + x + y + z
  }
}
class J extends I {
  n(x, ...r) {
    return "J" + r.length
  }
}
const is: I[] = [new I(), new J()]
for (const i of is) console.log(i.n(1))
for (const i of is) console.log(i.n(1, 10))

// the rows still answer the same thing called directly
console.log(new A().f(1), new B().f(1, 2), new G().k(3))
