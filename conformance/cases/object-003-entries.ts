// T-09.b (v0.4.0) — Object.entries(obj) returns `Array<Array<Any>>`
// where each inner is `[key, value]`. Compile-time unfold uses
// the static struct_layouts; outer holds heap pointers (regular
// 8-byte slot stride), inner uses arr_alloc_any (16-byte tagged
// slots). End-to-end indexed access on entries[i][0..1] works
// through the T-10.d.i boxed-Any read.

type Pair = { a: i64, b: i64 }
let o: Pair = { a: 1, b: 2 }
let entries = Object.entries(o)
console.log(entries.length)
console.log(entries[0][0])
console.log(entries[0][1])
console.log(entries[1][0])
console.log(entries[1][1])

// Mixed-type fields — verifies per-field tag dispatch in T-09.b
// codegen (i64 / f64 / bool / string each get the matching
// ANY_* tag).
type Mixed = { n: i64, x: f64, ok: boolean, label: string }
let m: Mixed = { n: 7, x: 1.5, ok: true, label: 'hello' }
let mEntries = Object.entries(m)
console.log(mEntries.length)
console.log(mEntries[0][0])
console.log(mEntries[0][1])
console.log(mEntries[1][0])
console.log(mEntries[1][1])
console.log(mEntries[2][0])
console.log(mEntries[2][1])
console.log(mEntries[3][0])
console.log(mEntries[3][1])
