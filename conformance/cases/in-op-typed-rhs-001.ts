// §13.10.1 — `in` is HasProperty on an ordinary object, and a
// non-Object rhs is a RUNTIME TypeError (step 5), not a compile
// reject. The rhs whitelist rejected `"get" in new Map()` — a
// program bun runs — while the identical receiver through an `any`
// binding answered.
const m = new Map<string, number>()
console.log("get" in m, "nope" in m, "hasOwnProperty" in m)

const s = new Set<number>()
const d = new Date(0)
const r = /a/g
console.log("has" in s, "getTime" in d, "test" in r, "lastIndex" in r)

const p = Promise.resolve(1)
console.log("then" in p, "nope" in p)

const w = new WeakMap()
console.log("get" in w, "deref" in new WeakRef({}))

const it = [1].values()
console.log("next" in it, "nope" in it)

const mi = new Map([[1, 2]]).entries()
console.log("next" in mi)

// A primitive rhs throws where it used to fail to build.
const n = 42
try {
  console.log("a" in (n as any))
} catch (e: any) {
  console.log("number rhs:", e instanceof TypeError)
}
const str = "abc"
try {
  console.log("a" in (str as any))
} catch (e: any) {
  console.log("string rhs:", e instanceof TypeError)
}

// An own key on one of the new property faces, and the shapes that
// already worked.
const mm: any = new Map()
mm.zz = 1
console.log("zz" in mm)
console.log(0 in [1, 2, 3], 5 in [1, 2, 3], "length" in [1])
class C {
  x = 1
}
console.log("x" in new C(), "y" in new C())
