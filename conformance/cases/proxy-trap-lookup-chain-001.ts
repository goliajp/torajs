// A trap is read off its handler by [[Get]] (§7.3.11 GetMethod): an own
// trap, one inherited from the handler's prototype, one behind a getter,
// and a name the handler holds as undefined or does not hold (forward to
// the target). A chain of proxies reads one trap per level, and every
// get's invariant check reads the target's own descriptor through the
// chain below it.
const log: string[] = []
const base: any = { v: 1, w: 2 }
let p: any = base
for (let i = 0; i < 300; i++) {
  p = new Proxy(p, {
    get(t: any, k: any, r: any) {
      return Reflect.get(t, k, r)
    },
  })
}
let s = 0
for (let i = 0; i < 3; i++) s += p.v + p.w
console.log(s, p.missing, "v" in p, Object.getOwnPropertyDescriptor(p, "v")?.value)

const protoHandler: any = {
  get(t: any, k: any) {
    log.push("inherited " + String(k))
    return t[k]
  },
}
const inherited: any = new Proxy({ a: 3 }, Object.create(protoHandler))
console.log(inherited.a)

const viaGetter: any = new Proxy(
  { b: 4 },
  {
    get get() {
      log.push("getter read")
      return (t: any, k: any) => t[k] * 10
    },
  },
)
console.log(viaGetter.b)

const undefTrap: any = new Proxy({ c: 5 }, { get: undefined, has: undefined })
console.log(undefTrap.c, "c" in undefTrap)

const gopd: any = new Proxy(
  { d: 6 },
  {
    getOwnPropertyDescriptor(t: any, k: any) {
      log.push("gopd " + String(k))
      return Reflect.getOwnPropertyDescriptor(t, k)
    },
  },
)
let q: any = gopd
for (let i = 0; i < 3; i++) {
  q = new Proxy(q, { get: (t: any, k: any, r: any) => Reflect.get(t, k, r) })
}
console.log(q.d)
const bad: any = new Proxy({}, { get: 5 })
try {
  console.log(bad.x)
} catch (e) {
  console.log(e instanceof TypeError)
}
console.log(log.join("; "))
