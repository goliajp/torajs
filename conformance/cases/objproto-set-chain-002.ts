// The same §10.1.9.2 chain, from the receivers that are not ordinary
// objects. A function, an array, a promise, a wrapper and a typed
// array each wrote straight into their own expando bag on an own
// miss, so nothing above them ever got asked — §10.2.4's restricted
// `caller` / `arguments` accessors on %Function.prototype% were the
// loudest case: `f.caller = {}` quietly minted an own key.

function mk(): any {
  return function () {}
}

// §10.2.4 — the restricted pair rejects through its %ThrowTypeError%
const f: any = mk()
try {
  f.caller = {}
  console.log("caller no-throw")
} catch (e: any) {
  console.log("caller", e.constructor.name)
}

// an inherited accessor one link up runs with the function receiver
let seen: any = null
Object.defineProperty(Function.prototype, "acc", {
  set(v: any) {
    seen = [this, v]
  },
  configurable: true,
})
const g: any = mk()
g.acc = 3
console.log(seen[0] === g, seen[1], Object.getOwnPropertyNames(g).join(","))

// an own entry shadows it, and freezing bars the create, not the walk
Object.defineProperty(g, "acc", { value: 1, writable: true, configurable: true })
g.acc = 5
console.log(g.acc)
const frozen: any = mk()
Object.freeze(frozen)
frozen.acc = 4
console.log(seen[0] === frozen, seen[1])

// an inherited non-writable data property rejects the strict assign
Object.defineProperty(Function.prototype, "ro", { value: 1, configurable: true })
const h: any = mk()
try {
  h.ro = 2
  console.log("ro no-throw")
} catch (e: any) {
  console.log("ro", e.constructor.name)
}

// two links up, from receivers with no [[Prototype]] slot of their own
let hits = 0
Object.defineProperty(Object.prototype, "root", {
  set(v: any) {
    hits += 1
  },
  configurable: true,
})
const recvs: any[] = [[1], Promise.resolve(1), new Number(1), new Uint8Array(2), mk()]
for (const r of recvs) r.root = 1
console.log(hits, recvs.map((r: any) => Object.getOwnPropertyNames(r).length).join(","))

// the own domains those lanes guard are untouched
const a: any = [1]
a.x = 1
a.x = 2
console.log(a.x, a.length)
const s: any = new String("ab")
try {
  s.length = 5
  console.log("length no-throw")
} catch (e: any) {
  console.log("length", e.constructor.name)
}
