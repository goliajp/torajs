// Dot-all DFA fast-path (chunk 10a `.` w/ s flag) — 100k iter.
// `/a.+c/s` with the `s` flag means `.` matches every byte
// including `\n`. chunk 10a wired `Op::AnyChar` to step every
// byte under `s` (without `s` it skips `\n` — Pike VM path).
const re = /a.+c/s
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = 'pre a\nmiddle\nc post ' + i.toString()
  const m = s.match(re)
  if (m !== null) total = total + m[0].length
}
console.log(total)
