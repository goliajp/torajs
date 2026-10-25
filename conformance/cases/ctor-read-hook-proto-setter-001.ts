// The `__proto__` accessor's setter, taken out of its descriptor or
// through `__lookupSetter__` with a run-time key, relinks whatever it
// is stored on and assigned through (B.2.2.1.2) with no `__proto__`
// spelled at the write. Either can put a program `valueOf` on the
// chain a constructor cell's ToPrimitive walks, so a coercing use of a
// run-time `constructor` read keeps the namespace statics.
const d: any = Object.getOwnPropertyDescriptor(Object.prototype, "__proto__")
const hook: any = {
  valueOf() {
    return 9
  },
}
const q: any = function () {
  return 0
}
Object.defineProperty(q, "relink", { set: d.set })
q.relink = hook
console.log(q + 1)
const name: string[] = ["__pro", "to__"]
const s: any = (Object.prototype as any).__lookupSetter__(name[0] + name[1])
const z: any = { a: 1 }
Object.defineProperty(z, "relink", { set: s })
z.relink = hook
console.log(z + 1)
const r: Record<string, number> = { a: 1 }
const p: string[] = ["constr", "uctor"]
console.log(r[p[0] + p[1]] + 1)
