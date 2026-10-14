// `Object.assign` copies the own fields of its sources; a source of a
// known layout reaches only the names it has, each with its own value's
// kind, and the typed reads of every other field keep their loads.
type Opt = { a?: number; b: number; s: string; t?: number }

const dst: Opt = { a: 1, b: 1.5, s: "d" }
const src = { b: 3, s: "src" }
Object.assign(dst, src)
console.log(dst.a, dst.b, dst.s, dst.t, "a" in dst, "t" in dst, JSON.stringify(dst))

const more = { s: "more" }
const num = { b: 7.5 }
Object.assign(dst, num, more)
console.log(dst.a, dst.b, dst.s, dst.t, JSON.stringify(dst))

const rows: Opt[] = []
for (let i = 0; i < 6; i++) rows.push(i % 2 === 0 ? { b: i, s: "r" } : { a: i, b: i, s: "r", t: i })
let total = 0
for (const r of rows) total = total + (r.a ?? 0) + r.b + (r.t ?? 0)
console.log(total, rows[0].b, rows[0].s, rows[0].a, rows[0].t)
