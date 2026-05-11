// V3-18 P2.4.c — anonymous object literal layouts. `let o =
// { x: 1, y: 2 }` no longer needs an explicit `type T = ...`
// declaration. ssa_lower auto-registers the layout on first
// occurrence; subsequent literals of the same shape reuse it
// (matching `layout_compatible`).
//
// Pre-fix tora panicked with "anonymous struct types not yet
// supported (P2.4.c MVP)".

let o = { x: 1, y: 2 }
console.log(o.x, o.y)

let p = { a: "hi", b: true }
console.log(p.a, p.b)

// Nested anon objects.
let nested = { inner: { val: 42 } }
console.log(nested.inner.val)

// Array of anon objects (same shape — reuses layout).
let arr = [{ a: 1 }, { a: 2 }, { a: 3 }]
console.log(arr.length, arr[0].a, arr[2].a)

// Mixed field types still work.
let mixed = { i: 5, s: "hi", b: false }
console.log(mixed.i, mixed.s, mixed.b)

// Existing `type T = { ... }` form still works (no regression).
type Pt = { x: number; y: number }
let q: Pt = { x: 10, y: 20 }
console.log(q.x, q.y)
