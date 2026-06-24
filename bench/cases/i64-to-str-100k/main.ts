// Pure Number.toString(int) substrate cost — no concat, no array,
// no regex. Isolates `__torajs_i64_to_str` (chunk 7.7 v2 step 12
// Round 2 attack #A2 / #A3 in .claude/rfcs/20260624-jsland-substrate-round2/
// decomposition.md). Expected stdout = sum of decimal-digit-counts
// of 0..99999 = 488890.
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = i.toString()
  total = total + s.length
}
console.log(total)
