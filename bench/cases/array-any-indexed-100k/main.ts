// Pure Array<Any> indexed read substrate cost — pre-built array,
// no allocation in the loop. Isolates Round 2 attacks A9 (unify
// __torajs_arr_get_any_tag + value) + A10 (SSA bounds-check
// elimination).
const n = parseInt(process.argv[2], 10)
const pool = parseInt(process.argv[3], 10)
const arr: any[] = []
for (let j = 0; j < pool; j = j + 1) arr.push('item' + j)
let total = 0
for (let i = 0; i < n; i = i + 1) {
  const s = arr[i % pool]
  total = total + s.length
}
console.log(total)
