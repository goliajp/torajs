// Round 3 isolation fixture F-R4: u flag + cp-range CharClass.
// `[A-Za-z]+/u` triggers the u-flag wire (continuation-byte advance,
// uflag dispatch) WITHOUT triggering chunk-10d's `utf8_class_expand`
// rewrite — `[A-Za-z]` is `u_props == 0`, no high-byte bits, no
// `negate` → `is_uflag_safe(cc)` returns true → `Op::Class` keeps
// cp-range form, DFA stays small.
//
// Purpose: isolate "is the regex-dfa-uflag-100k gap from the u-flag
// wire OR from `\p{L}` → Alt-of-Concat expansion?"
// Sibling-band ratio (~1.75-2×) here = gap lives in `\p{L}` expansion.
const re = /[A-Za-z]+/u
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const s = '  Hello42 word  ' + i.toString()
  const m = s.match(re)
  if (m !== null) total = total + m[0].length
}
console.log(total)
