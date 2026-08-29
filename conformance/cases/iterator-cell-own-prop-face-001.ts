// §23.1.5.1 / §22.1.5.1 / §24.1.5.1 / §24.2.5.1 / §27.1.4.x all mint
// ORDINARY objects: the source, the cursor and the captured callback
// are internal state, so a name key on an iterator is an ordinary own
// property and must land somewhere.
const ai: any = [1, 2].values()
ai.zz = 3
console.log(ai.zz, ai.next().value, Object.keys(ai), Object.getOwnPropertyNames(ai))
console.log("zz" in ai, JSON.stringify(Object.getOwnPropertyDescriptor(ai, "zz")))
delete ai.zz
console.log(ai.zz, "zz" in ai, ai.next().value)

const si: any = "ab"[Symbol.iterator]()
si.tag = "s"
console.log(si.tag, si.next().value)

const mi: any = new Map([[1, 2]]).entries()
mi.tag = "m"
console.log(mi.tag, JSON.stringify(mi.next().value))

const xi: any = new Set([7]).values()
xi.tag = "x"
console.log(xi.tag, xi.next().value)

const h: any = [1, 2, 3].values().map((x: number) => x * 2)
h.tag = "h"
console.log(h.tag, h.next().value, h.toArray())

const sym = Symbol("k")
const k: any = [1].keys()
k[sym] = 9
console.log(k[sym], Object.getOwnPropertySymbols(k).length)

Object.defineProperty(k, "hidden", { value: 4, enumerable: false })
console.log(k.hidden, Object.keys(k), Object.getOwnPropertyNames(k).sort())

let total = 0
for (let i = 0; i < 500; i++) {
  const it: any = [i].values()
  it.i = i
  total += it.i
}
console.log(total)
