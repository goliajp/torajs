// rotation 191 — JSON.stringify walks a Tag::Obj struct cell reached
// through the any-lane. Before the fix the write_cell arm had no
// Tag::Obj case; struct instances (nominal or anonymous with a
// class_tag) fell through to the `_ => {}` catch-all and printed as
// `{}`, even though the same field was readable via `.v`.
//
// The fix routes Tag::Obj through __torajs_struct_layout_lookup +
// __torajs_struct_field_name/read_anyv (same helpers Object.keys /
// values / entries any-lane arms use).

// Isolated obj literal — nominal-typed by inference, already worked.
const iso = { v: 42 }
console.log('iso:', JSON.stringify(iso))

// Typed array of a typed struct — already worked.
type Item = { v: number }
const typed: Item[] = []
typed.push({ v: 3 })
console.log('typed:', JSON.stringify(typed))

// any[] holding obj literals — the failing case. `direct` shows the
// same struct printed straight out of the array behaves like the
// wrapped-in-array case.
const items: any[] = []
items.push({ v: 1 })
items.push({ v: 2 })
console.log('via-arr:', JSON.stringify(items))
console.log('direct:', JSON.stringify(items[0]))

// Field access parity — always worked, keeps a floor for regressions.
console.log('access:', items[0].v, items[1].v)

// Multi-field struct in any[] to guarantee we walk more than the
// first slot.
const items2: any[] = []
items2.push({ a: 1, b: 'x', c: true })
console.log('multi:', JSON.stringify(items2[0]))
