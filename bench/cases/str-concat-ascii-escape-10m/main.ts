// str-concat-ascii-escape-100k at 10M iterations (steady state on every runtime).
// Escaping twin of str-concat-ascii-100k: the same `prefix + tail` per
// iteration, but every result is stored into a ring that outlives the
// loop and is read by index, so the concat must be built every time.
// The tail comes from argv so the concat cannot be folded at compile time.
const prefix = '  Hello42 word  '
const tail = process.argv[3]
const n = parseInt(process.argv[2], 10)
const ring: string[] = ['', '', '', '', '', '', '', '']
let total = 0
for (let i = 0; i < n; i = i + 1) {
  const s = prefix + tail
  ring[i & 7] = s
  total = total + s.charCodeAt(i & 15)
}
console.log(total)
console.log(ring[(n - 1) & 7])
