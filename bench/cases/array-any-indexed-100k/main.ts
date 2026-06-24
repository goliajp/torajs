// Pure Array<Any> indexed read substrate cost — pre-built array,
// no allocation in the loop. Isolates Round 2 attacks A9 (unify
// __torajs_arr_get_any_tag + value) + A10 (SSA bounds-check
// elimination) per .claude/rfcs/20260624-jsland-substrate-round2/
// decomposition.md §10.
const arr: any[] = []
for (let j = 0; j < 100; j = j + 1) arr.push('item' + j)
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = arr[i % 100]
  total = total + s.length
}
console.log(total)
