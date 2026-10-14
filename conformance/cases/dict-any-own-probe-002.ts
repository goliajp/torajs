// The own-entry write probes once; every path an absent key needs
// still runs: an inherited setter on Object.prototype, the Annex B
// `__proto__` setter, a non-extensible or frozen receiver, an own
// accessor entry, and a symbol key.
const names: string[] = ["zz", "__proto__", "w", "acc", "nw"]
let seen = 0
Object.defineProperty(Object.prototype, "zz", {
  set(v: any) {
    seen = seen + 1
    ;(this as any).zzSeen = v
  },
  get() {
    return "inherited"
  },
  configurable: true,
})
const a: any = {}
a[names[0]] = 7
console.log(seen, a.zzSeen, a[names[0]], Object.keys(a).join(","))
Object.defineProperty(a, "zz", { value: 1, writable: true, enumerable: true, configurable: true })
a[names[0]] = 8
console.log(seen, a[names[0]])
delete (Object.prototype as any).zz
// __proto__ through a dynamic key
const parent: any = { fromParent: 42 }
const b: any = {}
b[names[1]] = parent
console.log(b.fromParent, Object.keys(b).length, Object.getPrototypeOf(b) === parent)
console.log(b[names[1]] === parent)
// own accessor entry
const c: any = {}
let stored = 0
Object.defineProperty(c, "acc", {
  get() {
    return stored * 2
  },
  set(v: any) {
    stored = v
  },
  configurable: true,
})
c[names[3]] = 21
console.log(stored, c[names[3]])
const ro: any = {}
Object.defineProperty(ro, "acc", {
  get() {
    return 3
  },
  configurable: true,
})
try {
  ro[names[3]] = 1
  console.log("no throw")
} catch (e) {
  console.log(e instanceof TypeError, ro[names[3]])
}
// non-writable own entry
const n: any = {}
Object.defineProperty(n, "nw", { value: 1, writable: false, enumerable: true })
try {
  n[names[4]] = 2
  console.log("no throw")
} catch (e) {
  console.log(e instanceof TypeError, n[names[4]])
}
// non-extensible: own entries still write, a new key throws
const x: any = { w: 1 }
Object.preventExtensions(x)
x[names[2]] = 2
console.log(x[names[2]])
try {
  x[names[0]] = 3
  console.log("no throw")
} catch (e) {
  console.log(e instanceof TypeError, x[names[0]])
}
// frozen: an own write throws and keeps the value
const f: any = Object.freeze({ w: 1 })
try {
  f[names[2]] = 5
  console.log("no throw")
} catch (e) {
  console.log(e instanceof TypeError, f[names[2]])
}
// symbol key
const s = Symbol("s")
const y: any = {}
y[s] = 1
y[s] = y[s] + 1
console.log(y[s], Object.getOwnPropertySymbols(y).length)
y[s] = undefined
console.log(y[s], s in y)
