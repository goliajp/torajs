// Releases of `any` values: cycles whose members lose references
// through the any lane (first with the count above one, then the
// last), Str cells shared by many any slots, substrings, literals and
// immediates. Every cycle must still reach the collector.

function churn(n: number): number {
  let seen = 0
  for (let i = 0; i < n; i = i + 1) {
    const a: any = { id: i }
    const b: any = { id: i + 1 }
    a.o = b
    b.o = a
    let extra: any = a
    extra = b
    seen = seen + extra.o.id - a.o.id
  }
  return seen
}
console.log(churn(20000))

const base = 'shared-' + 'string-' + 'value'
const words: any[] = []
for (let i = 0; i < 1000; i = i + 1) {
  let w: any = base
  const part: any = base.slice(i % 7, 12)
  if (i % 3 === 0) w = part
  words.push(w)
  const lit: any = 'literal'
  const num: any = i
  words.push(lit + num)
}
let total = 0
for (const w of words) total = total + w.length
console.log(words.length, total, base.length)
words.length = 0
console.log(base)
