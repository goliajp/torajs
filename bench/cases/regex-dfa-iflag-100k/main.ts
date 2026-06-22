// Case-insensitive DFA fast-path (chunk 8.7 RE_FLAG_I) — 100k iter.
// `/hello/i` uses the DFA byte-step with ASCII case-fold helper
// (no Pike VM fallback). Pre-chunk-8.7 this took the slow path.
const re = /hello/i
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = 'before HELLO world ' + i.toString()
  const m = s.match(re)
  if (m !== null) total = total + m[0].length
}
console.log(total)
