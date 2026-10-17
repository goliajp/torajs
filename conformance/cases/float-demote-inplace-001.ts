// A sum whose loop body builds a string that the loop's throw exits
// still hold, so the loop cannot be copied: the sum runs as an integer
// with the f64 path folded into the same loop.
const n = parseInt(process.argv.length > 99 ? '3' : '4000', 10)
const tail = String(process.argv.length > 99 ? 'zz' : 'xy')
const prefix = '  Hello42 word  '
const ring: string[] = ['', '', '', '', '', '', '', '']
let total = 0
for (let i = 0; i < n; i = i + 1) {
  const s = prefix + tail
  ring[i & 7] = s
  total = total + s.charCodeAt(i & 15)
}
console.log(total, ring[(n - 1) & 7])
console.log(total + 1, total * 2, total > 301499, -total)
