// §10.1.9.2 OrdinarySet does not care that the compiler knew the
// receiver's shape. A store through a statically-known Map / Set /
// Date / RegExp / string / symbol / bigint receiver used to be a
// COMPILE error, while the same program spelled through an `any`
// binding stored fine. Read side: rotation 527.
const m = new Map<string, number>([["k", 1]])
;(m as any).zz = 7
console.log((m as any).zz, m.get("k"), m.size)

const s = new Set<number>([1])
;(s as any).zz = "set"
console.log((s as any).zz, s.has(1))

const d = new Date(0)
;(d as any).zz = true
console.log((d as any).zz, d.getTime())

const r = /a/g
;(r as any).zz = 3
r.lastIndex = 2
console.log((r as any).zz, r.lastIndex, r.source)

const t = new Uint8Array([1, 2])
;(t as any).zz = 4
console.log((t as any).zz, t[1])

// A primitive receiver has no place to put it — §10.1.9.2 through
// ToObject on a temporary, so strict-mode assignment throws. Same
// answer as the `any`-binding spelling has always given.
const n = 5
try {
  ;(n as any).zz = 1
} catch (e) {
  console.log("threw", (e as Error).constructor.name)
}
const str = "hi"
try {
  ;(str as any).zz = 1
} catch (e) {
  console.log("threw", (e as Error).constructor.name)
}
const g = 1n
try {
  ;(g as any).zz = 1
} catch (e) {
  console.log("threw", (e as Error).constructor.name)
}

// The value of an assignment expression is its rhs, on this lane too.
const m2 = new Map<string, number>()
const got = ((m2 as any).zz = 9)
console.log(got, (m2 as any).zz)
