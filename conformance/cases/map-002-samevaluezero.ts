// P6.1 — Map SameValueZero key equality edge cases. The key
// comparison in §23.1.3 follows the SameValueZero algorithm
// (§7.2.10) which has three notable corners: NaN equals NaN,
// +0 equals -0, and undefined / null are distinct from each other
// and from numeric zero. We assert each.

let m: Map = new Map()

// NaN as a key — SameValueZero treats it equal to itself, so
// re-setting under NaN should overwrite rather than insert a
// duplicate.
let nan1 = 0 / 0
let nan2 = Math.sqrt(-1)
m.set(nan1, 'first-nan')
m.set(nan2, 'second-nan')
console.log(m.size)
console.log(m.has(nan1))
console.log(m.has(nan2))
console.log(m.has(0 / 0))

// undefined / null as keys — both are valid Map keys per spec
// but they are distinct from each other.
m.set(undefined, 'undef-val')
m.set(null, 'null-val')
console.log(m.size)
console.log(m.has(undefined))
console.log(m.has(null))

// Delete the NaN + undef + null in turn.
console.log(m.delete(0 / 0))
console.log(m.delete(undefined))
console.log(m.delete(null))
console.log(m.size)
// +0 / -0 collapse case is a P12 follow-up — tora's literal-tier
// integer (0 / -0) hashes under ANY_I64 while a `let : number`
// annotation hits ANY_F64, so the cross-tag SameValueZero corner
// needs the broader Number IEEE-754 substrate (`number` literals
// always f64, integer narrowing only when provably safe).
