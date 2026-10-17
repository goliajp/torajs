// The same sum started just below 2^53: it crosses part way, and from
// there on odd code units round exactly as the f64 sum does.
const n = parseInt(process.argv.length > 99 ? '3' : '4000', 10)
const tail = String(process.argv.length > 99 ? 'zz' : 'xy')
const prefix = '  Hello42 word  '
const ring: string[] = ['', '', '', '', '', '', '', '']
let big = 9007199254740991 - 200000
for (let i = 0; i < n; i = i + 1) {
  const s = prefix + tail
  ring[i & 7] = s
  big = big + s.charCodeAt(i & 15)
}
console.log(big, big > 9007199254740991, big - 9007199254740991, ring[3])
