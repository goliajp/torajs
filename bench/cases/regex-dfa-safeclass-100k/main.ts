// u-flag safe class DFA fast-path (chunk 10c) — 100k iterations.
// `/[A-Za-z0-9]+/` is a u-flag-safe character class (ASCII range +
// bit set fits the byte-step's CharClass.test() without UTF-8
// expansion). chunk 10c lifted the u-flag blocker for this shape;
// chunk 10d covers the unsafe-class case via `utf8_class_expand`.
const re = /[A-Za-z0-9]+/
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = '  Hello42 world  ' + i.toString()
  const m = s.match(re)
  if (m !== null) total = total + m[0].length
}
console.log(total)
