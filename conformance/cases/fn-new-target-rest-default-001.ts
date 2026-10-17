// A plain function that reads new.target is constructed through a value:
// its rest parameter and its parameter defaults see the construct's
// arguments, and new.target is the function itself.
function Rest(this: any, ...xs: number[]) {
  this.isNew = new.target === Rest
  this.n = xs.length
  this.sum = xs.reduce((a, b) => a + b, 0)
}
function Mixed(this: any, head: string, ...tail: string[]) {
  this.kind = typeof new.target
  this.text = head + ":" + tail.join(",")
}
function Def(this: any, a: number = 5, b: string = "d") {
  this.kind = typeof new.target
  this.a = a
  this.b = b
}
function DefUntyped(this: any, a = 7) {
  this.t = new.target === DefUntyped
  this.a = a
}
const R: any = Rest
const M: any = Mixed
const D: any = Def
const U: any = DefUntyped
const r1 = new R(1, 2, 3)
console.log(r1.isNew, r1.n, r1.sum)
const r0 = new R()
console.log(r0.isNew, r0.n, r0.sum)
const m = new M("h", "x", "y")
console.log(m.kind, m.text)
const m1 = new M("only")
console.log(m1.kind, m1.text)
const d0 = new D()
console.log(d0.kind, d0.a, d0.b)
const d1 = new D(1)
console.log(d1.kind, d1.a, d1.b)
const d2 = new D(undefined, "z")
console.log(d2.kind, d2.a, d2.b)
const u0 = new U()
console.log(u0.t, u0.a)
const u1 = new U(9)
console.log(u1.t, u1.a)
const plain: any = {}
Rest.call(plain, 4, 5)
console.log(plain.isNew, plain.n, plain.sum)
Def.call(plain, 2)
console.log(plain.kind, plain.a, plain.b)
console.log(r1 instanceof Rest, d1 instanceof Def)
