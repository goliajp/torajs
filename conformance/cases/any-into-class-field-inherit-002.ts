// An object is handed as it is to a slot of any type its shape fits:
// an `any` value stored into a field through one type is read back as
// that value through the others. Each field is stored through one type
// only, so each path stands on its own.
class C { x = 1; y = 2 }
class D { y = 30; x = 40 }
interface I { i: C }
interface I2 extends I { n: number }
type B = { s: C }
type A = { s: C; n: number }
class K { k: C = new C(); z = 1 }
class L {
  l: C = new C()
  z = 1
  readL() { return [this.l.x, this.l.y].join(" ") }
}
function stI(o: I, v: any) { o.i = v }
function rdI2(o: I2) { return [o.i.x, o.i.y].join(" ") }
function stS(o: B, v: any) { o.s = v }
function rdA(o: A) { return [o.s.x, o.s.y].join(" ") }
function rdK(o: { k: C }) { return [o.k.x, o.k.y].join(" ") }
function stL(o: { l: C }, v: any) { o.l = v }
const vals: any[] = [new D(), { x: 5 }, { y: 6, x: 7 }]
for (const v of vals) {
  // through an interface, read through the one extending it
  const o: I2 = { i: new C(), n: 1 }
  stI(o, v)
  console.log("interface", rdI2(o), o.i === v)
  // through a shorter declared shape, read through a longer one
  const a: A = { s: new C(), n: 1 }
  stS(a, v)
  console.log("shape", rdA(a), a.s === v)
  // through the class, read through an object type literal
  const k = new K()
  k.k = v
  console.log("class to literal", rdK(k))
  // through an object type literal, read by the class's method
  const l = new L()
  stL(l, v)
  console.log("literal to class", l.readL())
}
