// `Object.assign` writes each source field into the target field of its
// name: a fraction reaching an integer-looking field, and an integer
// reaching a fractional one, both read back as the number written.
type T = { b: number; s: string }
const dst: T = { b: 1, s: "x" }
const src = { b: 2.5 }
Object.assign(dst, src)
console.log(dst.b, dst.b + 1, JSON.stringify(dst))

const f = { b: 1.5, n: 7 }
const ints = { b: 3, n: 4 }
Object.assign(f, ints)
console.log(f.b, f.b * 2, f.n, JSON.stringify(f))

const g = { v: 0, w: 2 }
Object.assign(g, { v: 0.25 }, { w: 9 })
console.log(g.v + g.w, JSON.stringify(g))
