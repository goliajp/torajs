// Word-boundary DFA fast-path (chunk 8.6 WBound) — 100k iter.
// `\bword\b` requires left/right byte class lookup. chunk 8.6's
// per-state `accept_before_byte` mask + 256-bit packed lookup
// resolves WBound entirely within the DFA byte-step (Pike VM
// fallback would re-evaluate the boundary per position).
const re = /\bword\b/
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = 'a word b xword wordy ' + i.toString()
  const m = s.match(re)
  if (m !== null) total = total + m[0].length
}
console.log(total)
