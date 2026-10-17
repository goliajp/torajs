// A Proxy releases its handler before its target, and a Map, an
// iterator helper and a Proxy leave the cycle-root buffer before their
// children go. Neither may change what survives: a level still held
// elsewhere keeps working after the chain above it is released, a
// handler shared by many levels outlives all but the last, and cycles
// through handler, target, Map entry or mapper closure are still
// reclaimed without disturbing live neighbours.
const log: string[] = []
const shared = {
  get(t: any, k: any, r: any): any {
    log.push("shared " + String(k))
    return Reflect.get(t, k, r)
  },
}

// proxy chain, alternating a shared handler and a per-level handler
// that also holds its target
let p: any = { x: 1, y: "leaf" }
let mid: any = null
for (let i = 0; i < 200; i++) {
  const t = p
  const own = {
    target: t,
    get(tt: any, k: any): any {
      return k === "level" ? i : tt[k]
    },
  }
  p = new Proxy(t, i % 2 === 0 ? shared : own)
  if (i === 99) mid = p
}
console.log("head", p.level, p.y)
p = null
console.log("mid", mid.level, mid.x, mid.y)
mid = null
console.log("shared gets", log.length > 0)

// handler and target are one object
let same: any = { get: (t: any, k: any) => (k === "who" ? "same" : t[k]), v: 7 }
let sp: any = new Proxy(same, same)
same = null
console.log("same", sp.who, sp.v)
sp = null

// a handler that holds its own proxy: a cycle through handler and target
let ring: any = null
for (let i = 0; i < 300; i++) {
  const h: any = {}
  const t: any = { i, prev: ring }
  const q = new Proxy(t, h)
  h.self = q
  t.self = q
  ring = q
}
console.log("ring", ring.i, ring.prev.i, ring.prev.prev.self.i)
ring = null

// revoked proxies in a chain
let rv: any = { k: "base" }
const revokers: any[] = []
for (let i = 0; i < 100; i++) {
  const r = Proxy.revocable(rv, {})
  revokers.push(r.revoke)
  rv = r.proxy
}
console.log("revocable", rv.k)
revokers[50]()
let threw = false
try {
  void rv.k
} catch (e) {
  threw = e instanceof TypeError
}
console.log("revoked mid", threw)
rv = null
revokers.length = 0

// Map chain with a middle level kept, and Maps that hold themselves
let mm = new Map<string, any>()
let keepMap: Map<string, any> | null = null
for (let i = 0; i < 3000; i++) {
  const x = new Map<string, any>()
  x.set("prev", mm)
  x.set("i", i)
  if (i % 7 === 0) x.set("self", x)
  if (i === 1500) keepMap = x
  mm = x
}
console.log("map head", mm.get("i"))
mm = new Map<string, any>()
console.log("map mid", keepMap!.get("i"), keepMap!.get("prev").get("i"), keepMap!.get("prev").get("prev").get("self") === keepMap!.get("prev").get("prev"))
let walked = 0
let cur: any = keepMap
while (cur && cur.has("i")) {
  walked++
  cur = cur.get("prev")
}
console.log("map walk", walked)
keepMap = null
cur = null

// iterator helpers: a middle helper keeps iterating after the head goes,
// and a mapper closure that captures its own helper
let it: any = [10, 20, 30, 40].values()
let midIt: any = null
for (let i = 0; i < 200; i++) {
  it = it.map((v: number) => v + 1)
  if (i === 99) midIt = it
}
console.log("helper head", it.next().value)
it = null
console.log("helper mid", midIt.next().value, midIt.next().value)
console.log("helper rest", [...midIt].join(","))
midIt = null
let selfIt: any = null
for (let i = 0; i < 100; i++) {
  const src = [i, i + 1].values()
  let me: any = null
  me = src.map((v: number) => (me ? v * 2 : v))
  selfIt = me
}
console.log("helper self", selfIt.next().value)
selfIt = null
console.log("done")
