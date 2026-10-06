// A Map<K, number> that reaches the any world through a cast
// (`(m as any).set(...)`) leaves the typed number value lane, so
// non-number values written through the any view, or through the
// typed binding with an any value, are stored and read back as bun
// does.

const m = new Map<string, number>()
m.set("a", 1)
try {
  ;(m as any).set("b", "text")
  console.log("set")
} catch (e) {
  console.log(e instanceof TypeError)
}
console.log(m.get("b"), m.size)
;(m as any).set("n", 7)
;(m as any).set("u", undefined)
console.log(m.get("n")! + 1, m.get("u"), m.size)

function widen(v: any): void {
  try {
    m.set("w", v)
    console.log("stored", m.get("w"))
  } catch (e) {
    console.log("refused", e instanceof TypeError)
  }
}
widen(2.5)
widen("7")
widen({})

// a typed Map nobody marked (a parameter's) still takes any value
function fill(p: Map<string, number>): void {
  ;(p as any).set("s", "ok")
}
let plain = new Map<string, number>()
fill(plain)
console.log(plain.get("s"))
