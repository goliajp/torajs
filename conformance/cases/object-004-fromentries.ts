// T-09.c (v0.4.0) — Object.fromEntries via caller-driven typing.
// `let o: Pair = Object.fromEntries(entries)` — ssa_lower's
// LetDecl arm detects the call and unfolds per the slot struct
// schema. MVP: entries are assumed in struct field declaration
// order (matches Object.entries round-trip output); per-field
// type-untag from the Any box.

type Pair = { a: i64, b: i64 }
let o: Pair = { a: 10, b: 20 }
let entries = Object.entries(o)
let back: Pair = Object.fromEntries(entries)
console.log(back.a)
console.log(back.b)

// Mixed-type round-trip — exercises the per-field tag→native
// untag dispatch in lower_fromentries (i64 / f64 / bool / string).
type Mixed = { n: i64, x: f64, ok: boolean, label: string }
let m: Mixed = { n: 7, x: 1.5, ok: true, label: 'hello' }
let mEntries = Object.entries(m)
let mBack: Mixed = Object.fromEntries(mEntries)
console.log(mBack.n)
console.log(mBack.x)
console.log(mBack.ok)
console.log(mBack.label)
