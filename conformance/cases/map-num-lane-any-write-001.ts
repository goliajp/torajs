// A fresh Map<K, number> marks its value kind, so a non-number value
// written through an any view is refused at the write with a
// TypeError instead of surfacing later as a wrong number in the typed
// read lane. bun accepts the write; this is the Map twin of the
// number[] push backstop (escape-any-003-objfield-loud), and the
// .expected is written by hand for that reason.

const m = new Map<string, number>()
m.set("a", 1)
try {
  ;(m as any).set("b", "text")
  console.log("not reached")
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
