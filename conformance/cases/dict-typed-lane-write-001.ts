// A dynamic-key write on a `Record<string, number>` follows
// §10.1.9.2 OrdinarySet whatever the declared value type says: an own
// accessor runs its setter (and its throw propagates), a frozen or
// non-writable entry refuses with a TypeError in strict code, a sealed
// object rewrites its keys but takes no new one, `__proto__` reaches
// the inherited setter, and a deleted key is inserted again at the end.
const ks: string[] = ["a", "b", "c", "__proto__", "s", "t"]
const d: Record<string, number> = {}
let seen = 0
Object.defineProperty(d, ks[4], {
  get() {
    return seen
  },
  set(v: number) {
    seen = seen + v
  },
  enumerable: true,
  configurable: true,
})
for (let i = 0; i < 4; i = i + 1) d[ks[4]] = (d[ks[4]] ?? 0) + 1
console.log(seen, d[ks[4]])
Object.defineProperty(d, ks[5], {
  set(_v: number) {
    throw new RangeError("no t")
  },
  configurable: true,
})
try {
  d[ks[5]] = 1
} catch (e) {
  console.log(e instanceof RangeError, (e as Error).message)
}
d[ks[0]] = 1
d[ks[1]] = 2
delete d[ks[0]]
d[ks[0]] = (d[ks[0]] ?? 10) + 1
console.log(Object.keys(d).join(","), d[ks[0]], d[ks[1]])
d[ks[3]] = 5
console.log(Object.getPrototypeOf(d) === Object.prototype, ks[3] in d, Object.keys(d).length)

const sealed: Record<string, number> = Object.seal({ a: 1 })
sealed[ks[0]] = sealed[ks[0]] + 1
try {
  sealed[ks[2]] = 3
} catch (e) {
  console.log("seal", e instanceof TypeError)
}
console.log(JSON.stringify(sealed))
const frozen: Record<string, number> = Object.freeze({ a: 1, b: 2 })
for (const k of ks.slice(0, 3)) {
  try {
    frozen[k] = 9
    console.log("wrote", k)
  } catch (e) {
    console.log("frozen", k, e instanceof TypeError)
  }
}
console.log(JSON.stringify(frozen))

const proto: Record<string, number> = { a: 100 }
const child: Record<string, number> = Object.create(proto)
child[ks[0]] = child[ks[0]] + 1
console.log(child[ks[0]], proto[ks[0]], Object.keys(child).join(","))
const counts: Record<string, number> = {}
for (let i = 0; i < 30; i = i + 1) {
  const k = ks[i % 3]
  counts[k] = (counts[k] ?? 0) + (i % 2 === 0 ? 1 : 0.5)
}
console.log(JSON.stringify(counts))
