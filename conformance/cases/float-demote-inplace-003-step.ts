// A constant step past 2^53 stops moving: 2^53 + 1 rounds back down.
const tail = String(process.argv.length > 99 ? 'zz' : 'xy')
const prefix = '  Hello42 word  '
const ring: string[] = ['', '', '', '', '', '', '', '']
let count = 9007199254740991 - 5
for (let i = 0; i < 20; i = i + 1) {
  const s = prefix + tail
  ring[i & 7] = s
  count = count + 1
}
console.log(count, count - 9007199254740991, ring[1])
