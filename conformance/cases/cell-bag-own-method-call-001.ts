// §10.1.8.1 OrdinaryGet does not care what internal state a receiver
// carries: a function stored as an own property is what `o.f()`
// calls, and an own name that collides with a builtin shadows it.
// Every shape below keeps its own face in a lazy bag.
const m: any = new Map<string, number>([["k", 1]])
m.zz = function () {
  return "map-own"
}
m.has = function () {
  return "shadowed"
}
console.log(m.zz(), m.has("k"), m.get("k"), m.size)

const s: any = new Set<number>([1])
s.zz = () => "set-own"
console.log(s.zz(), s.has(1))

const d: any = new Date(0)
d.getTime = function () {
  return 42
}
console.log(d.getTime(), d.getFullYear() > 1960)

const r: any = /a/g
r.zz = function () {
  return "re-own"
}
console.log(r.zz(), r.source)

const it: any = [1, 2].values()
it.zz = function () {
  return "iter-own"
}
console.log(it.zz(), it.next().value)

const pr: any = Promise.resolve(1)
pr.zz = function () {
  return "promise-own"
}
console.log(pr.zz())

const ta: any = new Uint8Array([7, 8])
ta.zz = function () {
  return "ta-own"
}
console.log(ta.zz(), ta[0])

const ab: any = new ArrayBuffer(4)
ab.zz = function () {
  return "ab-own"
}
console.log(ab.zz(), ab.byteLength)

class C {
  x = 1
}
const o: any = new C()
o.zz = function () {
  return "struct-own"
}
console.log(o.zz(), o.x)

// A defineProperty-installed own entry is the same own property.
const m2: any = new Map()
Object.defineProperty(m2, "zz", { value: () => "defined", configurable: true })
console.log(m2.zz())

// A stored non-callable is the §13.3.6 TypeError, not a fallthrough
// to the builtin surface.
const m3: any = new Map()
m3.has = 5
try {
  m3.has(1)
} catch (e) {
  console.log("threw", (e as Error).constructor.name)
}

// A wrapper's bag is the same surface — this one segfaulted before
// the probe learned to read the slot's tag before its payload.
const w: any = new Number(1)
w.zz = 5
try {
  w.zz()
} catch (e) {
  console.log("threw", (e as Error).constructor.name)
}

// An own entry storing undefined shadows the builtin too: the call
// is the resolved-not-callable TypeError, not the builtin `has`.
const m4: any = new Map([[1, "v"]])
m4.has = undefined
try {
  m4.has(1)
} catch (e) {
  console.log("threw", (e as Error).constructor.name)
}

// §13.3.6 — an accessor own entry runs its getter with the receiver
// as `this`, then calls what it answered.
const m5: any = new Map()
Object.defineProperty(m5, "f", {
  get(this: any) {
    const self = this
    return () => (self === m5 ? "acc-recv" : "acc-other")
  },
  configurable: true,
})
console.log(m5.f())

// A throwing getter aborts the call — its throw is what escapes,
// not a not-callable TypeError raised after it.
const m6: any = new Map()
Object.defineProperty(m6, "g", {
  get() {
    throw new RangeError("from getter")
  },
  configurable: true,
})
try {
  m6.g()
} catch (e) {
  console.log("threw", (e as Error).constructor.name, (e as Error).message)
}

// §13.3.6 EvaluateCall binds the holder as `this` — the bag is the
// receiver's storage, never the receiver.
const m7: any = new Map()
m7.who = function (this: any) {
  return this === m7
}
console.log(m7.who())

class D {
  y = 2
}
const o2: any = new D()
o2.who = function (this: any) {
  return this === o2 && this.y === 2
}
console.log(o2.who())
