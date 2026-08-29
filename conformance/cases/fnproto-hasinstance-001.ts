// §20.2.3.6 — %Function.prototype% OWNS the default `instanceof`
// handler. tr had the behaviour it stands for (the operator fell
// through to the ordinary prototype walk) but not the property:
// `f[Symbol.hasInstance]` answered undefined and
// getOwnPropertySymbols(Function.prototype) was empty.

const syms: any[] = Object.getOwnPropertySymbols(Function.prototype)
console.log(syms.length, syms[0] === Symbol.hasInstance)

// {W,E,C} are all false — the one fully locked own property here
const d: any = Object.getOwnPropertyDescriptor(Function.prototype, Symbol.hasInstance)
console.log(typeof d.value, d.writable, d.enumerable, d.configurable)
console.log(d.value.name, d.value.length)

// every function reaches it, and it is one function object
function F() {}
const f: any = F
const g: any = function () {}
console.log(f[Symbol.hasInstance] === d.value, g[Symbol.hasInstance] === d.value)

// the operator's answer is unchanged, now that the handler IS the walk
class A {}
class B extends A {}
const b: any = new B()
const inst: any = new (F as any)()
console.log(
  [
    b instanceof A,
    b instanceof B,
    ({} as any) instanceof A,
    inst instanceof F,
    (5 as any) instanceof F,
    (null as any) instanceof F,
  ].join(","),
)

// called directly, its step 1 is IsCallable(this) — `false`, not a throw
const h: any = d.value
console.log(h.call(null, {}), h.call({}, {}), h.call(F, inst), h.call(F, {}))

// a custom handler still wins: it is nearer on the chain
class C {
  static [Symbol.hasInstance](v: any) {
    return v === 7
  }
}
console.log((7 as any) instanceof (C as any), (new C() as any) instanceof (C as any))
