// `for (const c of "abc") chars.push(c)` — the for-of-str loop
// binds `c` as `Substr` (16-byte view into the source string),
// but `chars: string[]` slots are 8-byte heap-Str pointers. Pre-
// fix the raw 16-byte Substr struct was poured into the 8-byte
// slot, and the next `arr_print_str` decoded the slot as a Str
// header — yielding garbage Unicode characters (`觘 / 䃈 / …`).
//
// Fix routes through `substr_to_owned` at the push site, with
// rc accounting skipped (the fresh owned Str is the sole owner).

const chars: string[] = []
for (const c of 'abc') chars.push(c)
console.log(chars)

// Same shape, but with a fn-local target — exercises the (a)
// local push path (above exercises the K.8 global path).
function collect(s: string): string[] {
  const out: string[] = []
  for (const c of s) out.push(c)
  return out
}
console.log(collect('hello'))

// Multi-byte source — surrogate-aware advance keeps each entry
// a single code-point view.
const cps: string[] = []
for (const c of '日本') cps.push(c)
console.log(cps)

// Mixed ASCII / multi-byte
console.log(collect('a日b'))
