// Anchored DFA fast-path (chunk 8.5 AnchorB) — 100k iterations.
// `^\w+` matches a leading word at byte 0 only; with chunk 8.5 the
// DFA's text-start closure resolves AnchorB without per-position
// retries (Pike VM fallback would scan from byte 0 + 1 + 2 ...).
const re = /^\w+/
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = 'hello world ' + i.toString()
  const m = s.match(re)
  if (m !== null) total = total + m[0].length
}
console.log(total)
