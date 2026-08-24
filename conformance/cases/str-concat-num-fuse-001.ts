// S1-A2 attack B1 — fused `str + number` concat (concat_num_fuse).
// Covers both spellings (explicit .toString() and implicit coerce),
// i64 and f64 lanes, digit edge cases, and a UTF-16 left operand.
let total = 0
for (let i = 0; i < 3; i = i + 1) {
  const s = 'row ' + i.toString()
  total = total + s.length
  console.log(s)
}
console.log(total)
console.log('neg ' + (-7).toString())
console.log('zero ' + (0).toString())
console.log('big ' + 9007199254740991)
console.log('implicit ' + 42)
console.log('' + 5)
const f = 0.5
console.log('frac ' + f)
console.log('fracs ' + f.toString())
const tenth = 0.1
console.log('tenth ' + tenth)
const big = 1e21
console.log('exp ' + big)
const nz = -0.0
console.log('negzero ' + nz)
console.log('nan ' + (0 / 0))
console.log('inf ' + (1 / 0))
console.log('ninf ' + (-1 / 0))
console.log('中文 ' + 7)
console.log('中文 ' + 0.25)
const parts = 'a,b'.split(',')
console.log(parts[0] + 1)
