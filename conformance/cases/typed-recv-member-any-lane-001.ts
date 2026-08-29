// Knowing the receiver's SHAPE at compile time is not a licence to
// answer the read there. Every one of these used to be a COMPILE
// error ("ssa-lower: member access on non-object Map") on a program
// bun runs: the receiver's static type had no arm, while the same
// value through an `any` binding read its own bag and its prototype
// chain.
;(Object.prototype as any).root = "root"

const m = new Map<string, number>()
m.set("a", 1)
console.log((m as any).zz, (m as any).root, m.get("a"), m.size)

const s = new Set<number>([1])
console.log((s as any).zz, (s as any).root, s.has(1))

const d = new Date(0)
console.log((d as any).zz, (d as any).root, d.getTime())

const r = /a(b)/g
console.log((r as any).zz, (r as any).root, r.source, r.lastIndex)

const str = "ab"
console.log((str as any).zz, (str as any).root, str.length, str.toUpperCase())

const sym = Symbol("k")
console.log((sym as any).zz, sym.description)

const big = 10n
console.log((big as any).zz, big.toString())

const mi = new Map([[1, 2]]).entries()
console.log((mi as any).zz, JSON.stringify(mi.next().value))

const ai = [1, 2].values()
console.log((ai as any).zz, ai.next().value)

const wm = new WeakMap()
console.log((wm as any).zz)

// The prototype face still wins over the root, and a spec method is
// still the method.
;(Map as any).prototype.fam = "family"
console.log((m as any).fam, m.get("a"))
delete (Object.prototype as any).root
