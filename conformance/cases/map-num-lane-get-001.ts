// Map<K, number> typed value lane: a const binding of a fresh
// `new Map<K, number>()` reads `get` as an f64 — a number, or the
// undefined sentinel on a miss — and every consumer of a miss must
// still see undefined; receivers outside the lane answer the same

const m = new Map<string, number>()
for (let i = 0; i < 20; i = i + 1) {
  const key = "k" + (i % 5)
  if (m.has(key)) {
    m.set(key, m.get(key)! + i)
  } else {
    m.set(key, i)
  }
}
console.log(m.size, m.get("k0"), m.get("k4"))

const miss = m.get("nope")
console.log(miss, typeof miss, miss === undefined, miss ?? -1)
console.log(m.get("nope") === undefined, typeof m.get("k1"), m.get())
console.log(`${m.get("k2")}|${m.get("zz")}`, "v=" + m.get("k3"))

const xs: number[] = [1, 2]
m.set("half", 0.5)
m.set("negz", -0)
m.set("nan", NaN)
m.set("big", 2147483647 + 10)
m.set("hole", xs[9])
console.log(m.get("half")! * 4, 1 / m.get("negz")!, m.get("nan"), m.get("big"))
console.log(m.get("hole"), typeof m.get("hole"), m.get("hole") === undefined)
console.log(Math.abs(m.get("half")!), -m.get("half")!, m.get("hole") ?? -2)

// number keys take the pair lane
const byNum = new Map<number, number>()
byNum.set(1, 10).set(2.5, 20)
console.log(byNum.get(1)! + byNum.get(2.5)!, byNum.get(3), byNum.get(-0))
byNum.set(0, 7)
console.log(byNum.get(-0))

// a miss handed back through a return still reads undefined
function lookup(k: string): number | undefined {
  const local = new Map<string, number>()
  local.set("a", 1)
  return local.get(k)
}
console.log(lookup("a"), lookup("b"))

// the value survives trailing arguments, upsert and delete
const t = new Map<string, number>()
t.set("a", 1, 99)
console.log(t.get("a"), t.getOrInsert("b", 2), t.get("b"), t.getOrInsert("a", 5))
t.delete("a")
console.log(t.get("a"), t.size)

// receivers outside the lane: a parameter, a let, an alias, a capture
function viaParam(p: Map<string, number>): number {
  return p.get("k0")! + 1
}
let later = new Map<string, number>()
later.set("x", 3)
const alias = m
const readBack = () => m.get("k1")
console.log(viaParam(m), later.get("x"), alias.get("k2"), readBack())

// an alias class that reaches an any slot keeps the any lane
const esc = new Map<string, number>()
const u: any = esc
u.set("s", "text")
esc.set("n", 4)
console.log(esc.get("s"), esc.get("n"), typeof esc.get("s"))
