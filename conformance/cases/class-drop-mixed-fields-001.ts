// A class holding, next to its own-type link, a closure, an any
// value and a Map: the per-class release fn releases each through
// the field's own kind of release, and the instance dies through it
// whether it leaves a local, a field or an any-typed slot.
class Cell {
  next: Cell | null = null
  fn: (x: number) => number
  anyv: any
  m: Map<string, number>
  v: number
  constructor(v: number) {
    this.v = v
    const k = v * 2
    this.fn = (x: number) => x + k
    if (v % 2 === 0) this.anyv = { a: v }
    else this.anyv = "s" + v
    this.m = new Map<string, number>()
    this.m.set("v", v)
  }
}
let acc = 0
for (let i = 0; i < 3000; i++) {
  const c = new Cell(i)
  c.next = new Cell(i + 1)
  const a: any = c
  const f = c.fn
  const g = c.next.fn
  acc += f(1) + g(2) + (c.m.get("v") ?? 0) + a.v
  if (i % 3 === 0) acc += typeof c.anyv === "string" ? 1 : 2
}
console.log(acc)
const keep = new Cell(4)
const viaAny: any = keep
const back = viaAny as Cell
const h = back.fn
console.log(h(3), back.m.get("v"), back.anyv.a, back.next === null)
