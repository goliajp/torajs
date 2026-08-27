// §10.2.11 — an argument the call omits binds `undefined`, whatever
// lane carries the call. The vtable slot and the sibling-class lane
// used to hand the body a short argv, so the missing parameter read
// whatever the caller had left in that register.
class A {
  f(x, y) {
    console.log("a", typeof y, y)
  }
}
class B extends A {
  f(x, y) {
    console.log("b", typeof y, y)
  }
}
const xs: A[] = [new A(), new B()]
for (const x of xs) x.f(1)

// two missing slots through the same stub
class C {
  g(x, y, z) {
    console.log("c", typeof y, typeof z)
  }
}
class D extends C {
  g(x, y, z) {
    console.log("d", typeof y, typeof z)
  }
}
const ys: C[] = [new C(), new D()]
for (const y of ys) y.g(1)

// unrelated classes sharing a name — the sibling-class lane
class P {
  h(x, y) {
    console.log("p", typeof y, y)
  }
}
class Q {
  h(x, y) {
    console.log("q", typeof y, y)
  }
}
new P().h(1)
new Q().h(1)
