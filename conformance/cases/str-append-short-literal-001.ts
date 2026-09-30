// `s = s + <short literal>` writes the literal into the slack of a
// cell nobody else holds. Every other holder must keep seeing the
// value it read, and the accumulator's width and contents must come
// out the same as a copy would give.

function pick(i: number): string {
  return i % 2 === 0 ? 'ab' : 'cde'
}

// grows through every capacity class, one byte at a time
let one = ''
for (let i = 0; i < 300; i = i + 1) {
  one = one + 'x'
}
console.log(one.length, one.charCodeAt(0), one.charCodeAt(299), one === 'x'.repeat(300))

// each literal length, against an accumulator of every residue
const pieces = ['1', '12', '123', '1234', '12345', '123456', '1234567', '12345678']
let sizes = ''
for (let start = 0; start < 9; start = start + 1) {
  let a = 'q'.repeat(start)
  a = a + '1'
  a = a + '12'
  a = a + '123'
  a = a + '1234'
  a = a + '12345'
  a = a + '123456'
  a = a + '1234567'
  a = a + '12345678'
  sizes = sizes + a.length + ':' + (a === 'q'.repeat(start) + pieces.join('')) + ' '
}
console.log(sizes)

// a second holder keeps the value it read
let kept = pick(0)
const before = kept
kept = kept + '!'
kept = kept + '?'
console.log(before, kept)

const snaps: string[] = []
let acc = ''
for (let i = 0; i < 20; i = i + 1) {
  acc = acc + '.'
  snaps.push(acc)
}
console.log(snaps[0], snaps[7], snaps[19].length, acc.length)

// a literal binding is never written through
const lit = 'lit'
let fromLit = lit
fromLit = fromLit + '+'
fromLit = fromLit + '+'
console.log(lit, fromLit)

// a parameter the caller still holds
function tail(start: string, n: number): string {
  let out = start
  for (let i = 0; i < n; i = i + 1) {
    out = out + ';'
  }
  return out
}
const base = pick(1)
console.log(tail(base, 5), base)

// a Latin-1 literal onto a UTF-16 accumulator, in place
let wide = '中'
for (let i = 0; i < 40; i = i + 1) {
  wide = wide + ', '
}
console.log(wide.length, wide.charCodeAt(0), wide.charCodeAt(1), wide.charCodeAt(80), wide.slice(0, 5))

// a UTF-16 literal onto a Latin-1 accumulator widens it once
let mixed = pick(0)
for (let i = 0; i < 40; i = i + 1) {
  mixed = mixed + '中文测试'
}
console.log(mixed.length, mixed.slice(0, 8), mixed.charCodeAt(161))

// a non-ASCII Latin-1 literal, then a reader that depends on the units
let accent = pick(1)
for (let i = 0; i < 10; i = i + 1) {
  accent = accent + 'é'
}
console.log(accent.length, accent, accent.toUpperCase(), accent.indexOf('é'), accent.charCodeAt(12))

// null and undefined accumulators
let nul = null as any as string
nul = nul + '-'
nul = nul + '-'
let und = undefined as any as string
und = und + '='
console.log(nul, und)

// pieces of a chain: a call result, then literals and numbers
let row = ''
for (let i = 0; i < 12; i = i + 1) {
  row = row + pick(i) + ',' + i + ';'
}
console.log(row.length, row)

// a view piece between literal pieces
const cols = 'alpha,beta,gamma,中文'.split(',')
let line = ''
for (let i = 0; i < cols.length; i = i + 1) {
  line = line + '[' + cols[i] + ']'
}
console.log(line.length, line)

// the accumulator appended to itself after a literal append
let twice = 'x'
for (let i = 0; i < 5; i = i + 1) {
  twice = twice + '-'
  twice = twice + twice
}
console.log(twice.length, twice)
