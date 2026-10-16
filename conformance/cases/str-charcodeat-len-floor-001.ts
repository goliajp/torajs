// A concat is at least as long as its left side: an index masked
// below a literal's length reads a code unit without the NaN arm, and
// one that can reach past it keeps the check.
const prefix = '  Hello42 word  '
const tail = String(process.argv.length > 99 ? 'zz' : 'x')
const wide = String(process.argv.length > 99 ? 'zz' : '\u{1F600}')

let total = 0
for (let i = 0; i < 1000; i = i + 1) {
  const s = prefix + tail
  total = total + s.charCodeAt(i & 15)
}
console.log(total)

// the right side widens the result to two-byte code units
let wsum = 0
for (let i = 0; i < 100; i = i + 1) {
  wsum = wsum + (prefix + wide).charCodeAt(i & 15)
}
console.log(wsum)

// mask 31 reaches past 17 units: NaN arrives
let nans = 0
let hits = 0
for (let i = 0; i < 64; i = i + 1) {
  const c = (prefix + tail).charCodeAt(i & 31)
  if (c !== c) {
    nans = nans + 1
  } else {
    hits = hits + c
  }
}
console.log(nans, hits)

// the literal on the right says nothing of the length
let short = 0
for (let i = 0; i < 32; i = i + 1) {
  const c = (tail + 'ab').charCodeAt(i & 3)
  short = short + (c === c ? c : 1000)
}
console.log(short)

// a literal alone, and a chain whose leftmost side is the literal
const lit = 'abcdefgh'
let chain = 0
for (let i = 0; i < 40; i = i + 1) {
  chain = chain + lit.charCodeAt(i & 7) + (prefix + tail + tail).charCodeAt(i & 15)
}
console.log(chain)

// an astral literal counts two code units: '\u{1F600}abc' is 5 long,
// so mask 3 stays inside and mask 7 reaches past it
const astral = '\u{1F600}abc'
let inside = 0
let past = 0
for (let i = 0; i < 16; i = i + 1) {
  inside = inside + (astral + tail).charCodeAt(i & 3)
  const c = (astral + tail).charCodeAt(i & 7)
  past = past + (c === c ? c : 1000)
}
console.log(inside, past)
