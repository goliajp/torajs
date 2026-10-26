// A field a subclass inherits is one slot with its parent's: an `any`
// value stored into it through one class (§10.1.9 OrdinarySet stores
// the value as it is) is read back as that value through the other.
// Each field is stored through one type only, so each direction stands
// on its own.
class C { x = 1; y = 2 }
class D { y = 30; x = 40 }
class H {
  a: C = new C()
  b: C = new C()
  g: C = new C()
  f: () => string = () => "f"
  readA() { return [this.a.x, this.a.y].join(" ") }
  callF() { return this.f() }
}
class H2 extends H {
  readB() { return [this.b.x, this.b.y].join(" ") }
}
class H3 extends H2 {}
function stB(h: H, v: any) { h.b = v }
function rdG(h: H) { return [h.g.x, h.g.y].join(" ") }
const vals: any[] = [new D(), { x: 5 }, { y: 6, x: 7 }]
for (const v of vals) {
  // stored through the subclass, read by the parent's method
  const a = new H2()
  a.a = v
  console.log("sub->parent", a.readA(), a.a === v)
  // stored through the parent's type, read by the subclass's method
  const b = new H2()
  stB(b, v)
  console.log("parent->sub", b.readB(), b.b.y)
  // stored through a grandchild, read through its grandparent's type
  const c = new H3()
  c.g = v
  console.log("grand", rdG(c))
}
const fs: any[] = [() => "g", function () { return "k" }]
for (const g of fs) {
  const a = new H3()
  a.f = g
  console.log("fn sub->parent", a.callF())
}
