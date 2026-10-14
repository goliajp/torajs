// A generic declaration's optional `number` member keeps its f64 slot
// in every instantiation: a literal that leaves it out owns no such
// property, one that spells it does, and the member's name is the
// declared one.
type Box<T> = { v?: number; t: T; w?: T }
const b: Box<string> = { t: "s" }
const c: Box<string> = { v: 2, t: "u", w: "w" }
console.log(b.v, "v" in b, "w" in b, c.v, "v" in c, c.w, JSON.stringify(b), JSON.stringify(c))
console.log(Object.keys(b).join(","), Object.keys(c).join(","), (b.v ?? 0) + (c.v ?? 0))
const d: Box<number> = { v: 1.5, t: 3 }
console.log(d.v, d.t, "v" in d, typeof d.v, d.v! * 2)

class Cell<T> {
  n?: number
  t: T
  constructor(t: T) {
    this.t = t
  }
}
const e = new Cell<string>("e")
console.log(e.n, "n" in e, typeof e.n, JSON.stringify(e))
e.n = 4
console.log(e.n, "n" in e, Object.keys(e).join(","))
