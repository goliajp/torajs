// A class whose field is an anonymous struct shape holding heap
// values: a string, a class instance, and — through a wider literal
// bound to a local first — an array the field's declared shape never
// names. The class releases through its per-class release fn, which
// hands the field to the runtime's tag-dispatched release; that walk
// must free everything the cell actually carries, on every iteration.
class Other {
  v: number
  constructor(v: number) { this.v = v }
}
class Holder {
  info: { name: string; child: Other }
  next: Holder | null = null
  constructor(i: number) {
    this.info = { name: "n" + i, child: new Other(i) }
  }
}
class Wide {
  info: { name: string; child: Other }
  peer: Other | null = null
  constructor(i: number) {
    const w = { name: "w" + i, child: new Other(i * 2), extra: [i, i, i] }
    this.info = w
  }
}
function score(info: { name: string; child: Other }): number {
  return info.name.length + info.child.v
}
function churn(n: number): number {
  let acc = 0
  for (let i = 0; i < n; i++) {
    const h = new Holder(i)
    if (i % 4 === 0) h.next = new Holder(i + 1)
    const w = new Wide(i)
    w.peer = h.info.child
    acc += score(h.info) + score(w.info)
    const nx = h.next
    if (nx !== null) acc += score(nx.info)
  }
  return acc
}
console.log(churn(3000))
const keep = new Holder(7)
keep.info = { name: "replaced", child: new Other(70) }
console.log(score(keep.info), keep.info.name)
