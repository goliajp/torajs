// Regex-driven substitution on a short string, 100k iterations.
const re = /a(b+)c/g
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = 'abbc xxx abc yyy abbbbc'
  const r = s.replace(re, 'XY')
  total = total + r.length
}
console.log(total)
