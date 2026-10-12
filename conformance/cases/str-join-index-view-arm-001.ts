// A `?:` / `??` / `||` / `&&` join of a string index read `s[i]`
// (a fresh view of one code unit) and a whole string: whichever arm
// runs, the join prints the right text, an out-of-range read takes
// the other side or prints `undefined`, and the string itself is
// read again after a few fresh strings were built.
function joins(c: boolean, n: number, i: number): number {
  const s = 'ab'.repeat(n)
  const a = c ? s[i] : s
  const b = s[i] ?? s
  const x = c ? s : s[i]
  const o = s[i] || s
  const y = s && s[i]
  console.log(a, b, x, o, y)
  console.log(c ? s[i] : s, c ? s : s[i])
  console.log(s[i] ?? s, s[i] || s, s && s[i])
  console.log(c ? s[i] : s[i + 1], s[i] ?? 'none', s[i] || '')
  const pad = ['x'.repeat(2), 'y'.repeat(2)]
  console.log(s, pad.join(''))
  return b.length + o.length
}
console.log(joins(true, 2, 1))
console.log(joins(false, 3, 0))
console.log(joins(true, 2, 9))
console.log(joins(false, 2, 9))
const t = 'cd'.repeat(2)
const u = t.length > 9 ? t[0] : t
const v = t[7] ?? t
const w = t[1] || t
console.log(u, v, w, t)
const pad3 = ['x'.repeat(2), 'y'.repeat(2)]
console.log(t, u, v, w, pad3.join(''))
let tot = 0
for (let k = 0; k < 200000; k = k + 1) {
  const z = k % 2 === 0 ? t[k % 4] : t
  tot = tot + z.length
}
console.log(tot)
