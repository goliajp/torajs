// Two-byte code units crossing 2^53.
const n = parseInt(process.argv.length > 99 ? '3' : '4000', 10)
const tail = String(process.argv.length > 99 ? 'zz' : 'xy')
const wide = 'a￿翿\u0001b￾䀁\u0003c老ÿād翽༏e'
const ring: string[] = ['', '', '', '', '', '', '', '']
let wsum = 9007199254740991 - 60000000
for (let i = 0; i < n; i = i + 1) {
  const s = wide + tail
  ring[i & 7] = s
  wsum = wsum + s.charCodeAt(i & 15)
}
console.log(wsum, wsum - 9007199254740991, ring[2].length)
