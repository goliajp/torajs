// The own face rides the same integrity gate every other lazy-bag
// receiver does: freezing a Map bars the create, and the assign
// ladder walks the [[Prototype]] chain before it mints an own key.
const m: any = new Map()
;(Object.prototype as any).viaChain = 1
console.log(m.viaChain, Object.keys(m))
m.viaChain = 2
console.log(m.viaChain, Object.keys(m))
delete (Object.prototype as any).viaChain

// §10.1.9.2 step 2 — an inherited SETTER runs instead of minting an
// own key, and an inherited data property does not.
let heard = ""
Object.defineProperty(Object.prototype, "viaSetter", {
  set(v: any) {
    heard = "setter:" + v
  },
  get() {
    return heard
  },
  configurable: true,
})
const d2: any = new Date(0)
d2.viaSetter = 9
console.log(heard, Object.keys(d2), d2.viaSetter)
const r2: any = /x/
r2.viaSetter = 10
console.log(heard, Object.getOwnPropertyNames(r2))
delete (Object.prototype as any).viaSetter

const f: any = new Set()
Object.freeze(f)
try {
  f.nope = 1
} catch (e: any) {
  console.log("frozen:", e instanceof TypeError)
}
console.log(f.nope, Object.isFrozen(f))

let total = 0
for (let i = 0; i < 500; i++) {
  const d: any = new Date(i)
  d.i = i
  total += d.i
}
console.log(total)
