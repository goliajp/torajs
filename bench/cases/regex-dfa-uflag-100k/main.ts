// u-flag unsafe class DFA fast-path (chunk 10d) — 100k iter.
// `/\p{L}+/u` (Unicode Letter property) under the u flag would
// pre-chunk-10d need code-point boundary awareness and fall back
// to Pike VM. chunk 10d's `utf8_class_expand` rewrites the class
// compile-time into a byte-level Alt-of-Concat over `byte_only`
// CharClass instructions; the DFA byte-step walks it verbatim.
const re = /\p{L}+/u
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = '  Hello42 word  ' + i.toString()
  const m = s.match(re)
  if (m !== null) total = total + m[0].length
}
console.log(total)
