// §10.1.8.1 OrdinaryGet / §7.3.11 HasProperty do not care which key
// domain they are walking: a symbol installed on %Object.prototype%
// is reachable from every receiver whose chain passes through it, and
// `in` answers the same chain the read does.
const ROOT = Symbol.for("root-chain")
const PATCH = Symbol.for("family-patch")
Object.defineProperty(Object.prototype, ROOT, { value: "from root", configurable: true })
Object.defineProperty(Array.prototype, PATCH, { value: "from Array.prototype", configurable: true })

const arr: any = [1]
const map: any = new Map()
const set: any = new Set()
const re: any = /x/
const date: any = new Date(0)
const iter: any = arr[Symbol.iterator]()
const fn: any = function () {}
const wrapper: any = new Number(1)

// Every non-dynobj receiver shape reaches the root.
for (const [name, v] of [
  ["array", arr], ["map", map], ["set", set], ["regexp", re],
  ["date", date], ["arrayIterator", iter], ["function", fn],
  ["numberWrapper", wrapper], ["plainObject", {} as any],
] as any[]) {
  console.log(name, "get:", v[ROOT], "in:", ROOT in v)
}

// A primitive receiver walks through its wrapper's prototype too.
console.log("shortString get:", ("ab" as any)[ROOT], "| number get:", (7 as any)[ROOT])

// A builtin prototype is itself a receiver with a chain.
console.log("Array.prototype:", (Array.prototype as any)[ROOT], ROOT in (Array.prototype as any))
console.log("Object.prototype:", (Object.prototype as any)[ROOT], ROOT in (Object.prototype as any))

// The family prototype shadows the root, and `in` sees the patch the
// read already saw.
console.log("family patch get:", arr[PATCH], "in:", PATCH in arr, "| on map:", map[PATCH], PATCH in map)

// The reified faces still win over the root: they are what the family
// prototype OWNS, one link nearer than %Object.prototype%.
console.log("iterator face:", typeof arr[Symbol.iterator], Symbol.iterator in arr, [...arr].join())

// An own entry storing undefined is present, and shadows the chain.
const shadowed: any = {}
Object.defineProperty(shadowed, ROOT, { value: undefined, configurable: true })
console.log("shadowed:", shadowed[ROOT], ROOT in shadowed)

// An explicit null [[Prototype]] cuts the chain on the symbol lane
// too: no family prototype, no root, nothing to inherit.
const bare: any = Object.create(null)
console.log("nullProto:", bare[ROOT], ROOT in bare)
