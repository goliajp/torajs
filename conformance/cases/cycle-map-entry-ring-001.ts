// The collector reached a Map's own-property bag but not its entry
// table, so `m.set("k", m)` — and every ring that runs through a
// stored value rather than a property — was invisible to it. A Map
// owns two references per entry, the key's and the value's, and both
// are edges the walk has to trial-decrement.
//
// Observable here: the walk runs over these rings without crashing
// and a live one survives whole. Whether the dead ones are reclaimed
// is a peak-RSS question, measured out of band.

class Node {
  name: string
  bag: Map<string, any>
  constructor(name: string) {
    this.name = name
    this.bag = new Map()
  }
}

let ok = 0
for (let i = 0; i < 200; i++) {
  // value edge
  const m = new Map<string, any>()
  m.set("self", m)
  if (m.get("self") === m) ok++

  // key edge
  const k = new Map<any, string>()
  k.set(k, "self")
  if (k.get(k) === "self") ok++

  // Set membership edge
  const s = new Set<any>()
  s.add(s)
  if (s.has(s)) ok++

  // through a class instance
  const n = new Node("n" + i)
  n.bag.set("owner", n)
  if (n.bag.get("owner") === n) ok++

  // two maps pointing at each other
  const a = new Map<string, any>()
  const b = new Map<string, any>()
  a.set("b", b)
  b.set("a", a)
  if (a.get("b") === b && b.get("a") === a) ok++
}
console.log(ok)

Bun.gc(true)
console.log("after gc")

// A held ring keeps every edge, including the string key that the
// collector never owns.
const live = new Map<string, any>()
live.set("self", live)
live.set("n", 7)
Bun.gc(true)
const back: any = live.get("self")
console.log(live.size, back === live, back.get("n"), [...live.keys()].join(","))
