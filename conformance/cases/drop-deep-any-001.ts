// Deep chains released through the runtime's tag-dispatched walk: an
// anonymous object type whose field holds the same type, an interface
// chain over two classes, a class whose `any` field holds the next
// instance, object literals and arrays nested through `any`, and class
// instances linked through an expando property. Each release takes
// the next one's count to zero, so a walk that nests one native frame
// per level overruns the stack; the depth must stay bounded.
type L = { v: number; next: L | null }
interface I {
  v: number
  next: I | null
}
class C implements I {
  v: number
  next: I | null = null
  constructor(v: number) { this.v = v }
}
class D implements I {
  v: number
  w = "d"
  next: I | null = null
  constructor(v: number) { this.v = v }
}
class F {
  v: number
  x: any = null
  constructor(v: number) { this.v = v }
}
class E {
  v = 7
}

function anon(n: number): L {
  let h: L = { v: 0, next: null }
  for (let i = 1; i < n; i++) h = { v: i, next: h }
  return h
}
function iface(n: number): I {
  let h: I = new C(0)
  for (let i = 1; i < n; i++) {
    const c: I = i % 2 === 0 ? new C(i) : new D(i)
    c.next = h
    h = c
  }
  return h
}
function anyField(n: number): F {
  let h = new F(0)
  for (let i = 1; i < n; i++) {
    const f = new F(i)
    f.x = h
    h = f
  }
  return h
}
function anyObj(n: number): any {
  let h: any = null
  for (let i = 0; i < n; i++) h = { v: i, next: h }
  return h
}
function anyArr(n: number): any[] {
  let a: any[] = []
  for (let i = 0; i < n; i++) a = [a, i]
  return a
}
function expando(n: number): any {
  let h: any = new E()
  for (let i = 0; i < n; i++) {
    const e: any = new E()
    e.prev = h
    h = e
  }
  return h
}

for (let round = 0; round < 2; round++) {
  let l: L | null = anon(1000000)
  console.log("anon", l.v, l.next!.v)
  l = null
  let i: I | null = iface(300000)
  console.log("iface", i.v, i.next!.v)
  i = null
  let f: F | null = anyField(300000)
  console.log("any field", f.v, f.x.v)
  f = null
  let o: any = anyObj(200000)
  console.log("any object", o.v, o.next.v)
  o = null
  let a: any[] = anyArr(200000)
  console.log("any array", a.length, a[1], a[0][1])
  a = []
  let e: any = expando(150000)
  console.log("expando", e.v, e.prev.v)
  e = null
}
console.log("done")
