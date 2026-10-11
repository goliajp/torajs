// A spread copies only the members its source owns (§7.3.25): a member
// the source's type declares optional is copied when the source has it,
// and a required one always is.
type O = { a?: number; b: number }
type R = { a: number }

const o: O = { b: 1 }
const r: R = { a: 3 }
const m = { a: "x", ...r }
console.log(m.a, typeof m.a, "a" in o)

type S = { a?: number }
const s: S = {}
const s2: S = { a: 4 }
const m2 = { a: "x", ...s }
console.log(m2.a, typeof m2.a)
const m3 = { a: "x", ...s2 }
console.log(m3.a, typeof m3.a)

type T = { a?: string | number }
const t: T = {}
const m4 = { a: undefined, ...t }
console.log(m4.a, "a" in m4, Object.keys(m4))
const m5 = { ...t, ...s2 }
console.log(m5.a, Object.keys(m5))
const m6 = { ...s, ...t }
console.log("a" in m6, Object.keys(m6), JSON.stringify(m6))

// a spread of a spread still lacks what the first source lacked
const q: O = { b: 3 }
const c1 = { ...q }
const c2 = { ...c1 }
console.log("a" in c1, "a" in c2, Object.keys(c2), JSON.stringify(c2))
const c3 = { ...q, b: 9 }
console.log(c3, Object.keys(c3))
const { b, ...rest } = o
console.log(b, rest, Object.keys(rest))

// a member past the 32 the cell's own set can name
type W = {
  f00?: number; f01?: number; f02?: number; f03?: number; f04?: number; f05?: number
  f06?: number; f07?: number; f08?: number; f09?: number; f10?: number; f11?: number
  f12?: number; f13?: number; f14?: number; f15?: number; f16?: number; f17?: number
  f18?: number; f19?: number; f20?: number; f21?: number; f22?: number; f23?: number
  f24?: number; f25?: number; f26?: number; f27?: number; f28?: number; f29?: number
  f30?: number; f31?: number; f32?: number; f33?: number; last: number
}
const w: W = {
  f00: 0, f01: 1, f02: 2, f03: 3, f04: 4, f05: 5, f06: 6, f07: 7, f08: 8, f09: 9,
  f10: 10, f11: 11, f12: 12, f13: 13, f14: 14, f15: 15, f16: 16, f17: 17, f18: 18,
  f19: 19, f20: 20, f21: 21, f22: 22, f23: 23, f24: 24, f25: 25, f26: 26, f27: 27,
  f28: 28, f29: 29, f30: 30, last: 99,
}
console.log("f31" in w, "f33" in w, "f30" in w, Object.keys(w).length, JSON.stringify(w).length)
w.f33 = 33
console.log("f33" in w, w.f33, Object.keys(w).length)
