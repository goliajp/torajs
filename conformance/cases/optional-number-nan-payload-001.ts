// A double read out of a buffer may wear any NaN payload, including the
// bit pattern the typed lane uses for an optional `number` member's
// `undefined`. Such a read is a NaN like any other: stored into the
// member it reads back as NaN, never as undefined, and the member stays
// an own property.
type Opt = { a?: number; b: number }
const buf = new ArrayBuffer(16)
const words = new Uint32Array(buf)
words[0] = 0x0000000a
words[1] = 0x7ffc0000
words[2] = 0
words[3] = 0x7ff80000
const f = new Float64Array(buf)
const v: number = f[0]
const p: Opt = { a: v, b: 1 }
console.log(p.a, typeof p.a, p.a === undefined, Number.isNaN(p.a as number), "a" in p)
p.a = f[0]
console.log(p.a, typeof p.a, p.a ?? 9, JSON.stringify(p))
const q: Opt = { b: 2 }
q.a = f[0]
console.log(q.a, typeof q.a, "a" in q)
const anyv: any = new DataView(buf).getFloat64(0, true)
q.a = anyv
console.log(q.a, typeof q.a, Object.values(q).length, Object.values(q).map((x) => typeof x).sort().join("|"))
const w: any = f[0]
console.log(w, typeof w, w === undefined, f[1], f.join(","))
