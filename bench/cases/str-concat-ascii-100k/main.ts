// Pure Str.+ ASCII concat substrate cost — pre-allocated literal
// LHS, 'x' STATIC_LITERAL RHS removes the i.toString() cost.
// Mirrors uflag-100k LHS length-class. Isolates Round 2 attacks
// A6 (empty-string early-return) + A7 (rope) per
// .claude/rfcs/20260624-jsland-substrate-round2/decomposition.md §10.
const prefix = '  Hello42 word  '
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = prefix + 'x'
  total = total + s.length
}
console.log(total)
