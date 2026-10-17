// Sums the in-place shape must leave alone or get exactly right: an
// empty loop, a -0 start, NaN / Infinity / fraction / huge starts, and
// a code-unit index past the string's end (NaN addends).
function empty(): number {
  const tail = String(process.argv.length > 99 ? 'zz' : 'xy')
  const prefix = '  Hello42 word  '
  const ring: string[] = ['', '', '', '', '', '', '', '']
  const n = parseInt(process.argv.length > 99 ? '3' : '0', 10)
  let t = 0
  for (let i = 0; i < n; i = i + 1) {
    const s = prefix + tail
    ring[i & 7] = s
    t = t + s.charCodeAt(i & 15)
  }
  return t
}
function negZero(): number {
  const tail = String(process.argv.length > 99 ? 'zz' : 'xy')
  const prefix = '  Hello42 word  '
  const ring: string[] = ['', '', '', '', '', '', '', '']
  const n = parseInt(process.argv.length > 99 ? '3' : '0', 10)
  let t = -0
  for (let i = 0; i < n; i = i + 1) {
    const s = prefix + tail
    ring[i & 7] = s
    t = t + s.charCodeAt(i & 15)
  }
  return t
}
function sum(start: number, n: number, k: number): number {
  const tail = String(process.argv.length > 99 ? 'zz' : 'xy')
  const prefix = '  Hello42 word  '
  const ring: string[] = ['', '', '', '', '', '', '', '']
  let t = start
  for (let i = 0; i < n; i = i + 1) {
    const s = prefix + tail
    ring[i & 7] = s
    t = t + s.charCodeAt(i & k)
  }
  return t
}
console.log(empty(), Object.is(empty(), 0), Object.is(negZero(), -0))
console.log(Object.is(sum(-0, 0, 15), -0), Object.is(sum(-0, 3, 15), -0), sum(NaN, 5, 15))
console.log(sum(Infinity, 5, 15), sum(-Infinity, 5, 15), sum(0.25, 5, 15), sum(0, 40, 31))
console.log(sum(1.7976931348623157e308, 3, 15), sum(-9007199254740993, 4000, 15))
