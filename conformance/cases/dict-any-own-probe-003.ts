// A dynamic-key read on an ordinary object answers the whole value
// from one own probe; absent keys still walk the prototype chain,
// an own accessor still runs its getter, and an own entry storing
// undefined still shadows the chain.
const ks: string[] = ["v", "u", "g", "inh", "missing", "toString", "0", "n"]
const proto: any = { inh: "from proto", u: "proto u" }
const o: any = Object.create(proto)
o[ks[0]] = 1.5
o[ks[1]] = undefined
Object.defineProperty(o, "g", {
  get() {
    return "getter " + typeof this
  },
  enumerable: true,
  configurable: true,
})
o[ks[6]] = "zero"
o[ks[7]] = null
console.log(o[ks[0]], o[ks[1]], o[ks[2]], o[ks[3]], o[ks[4]])
console.log(typeof o[ks[5]], o[ks[6]], o[0], o[ks[7]])
console.log(ks[1] in o, o.hasOwnProperty(ks[1]), o.hasOwnProperty(ks[3]))
// every value shape read back through the same dynamic key
const vals: any[] = [0, -0, 1, 2147483647, 2147483648, -1.25, NaN, true, false, null, undefined, "s", "longer string", [1, 2], { a: 1 }]
const box: any = {}
const kk: string = ks[0]
for (let i = 0; i < vals.length; i = i + 1) {
  box[kk] = vals[i]
  const r = box[kk]
  console.log(i, typeof r, JSON.stringify(r), Object.is(r, vals[i]))
}
// a getter on Object.prototype answers an absent own key
Object.defineProperty(Object.prototype, "pg", {
  get() {
    return "proto getter"
  },
  configurable: true,
})
const e: any = {}
const kpg: string = ks.length > 0 ? "pg" : "x"
console.log(e[kpg])
try {
  e[kpg] = undefined
  console.log("no throw")
} catch (err) {
  console.log(err instanceof TypeError, e[kpg], Object.keys(e).length)
}
Object.defineProperty(e, "pg", { value: undefined, writable: true, enumerable: true, configurable: true })
console.log(e[kpg], Object.keys(e).join(","))
delete (Object.prototype as any).pg
// symbol key read
const sym = Symbol("k")
const w: any = {}
console.log(w[sym])
w[sym] = 3
console.log(w[sym])
w[sym] = undefined
console.log(w[sym], sym in w)
// null-prototype object
const np: any = Object.create(null)
np[ks[0]] = 4
console.log(np[ks[0]], np[ks[5]])
