// The concat's right operand alternates between two argv strings, so it
// must be built every iteration; every result is stored into a ring that
// outlives the loop and is read by index.
const prefix = '  Hello42 word  '
const tails: string[] = [process.argv[3], process.argv[4]]
const n = parseInt(process.argv[2], 10)
const ring: string[] = ['', '', '', '', '', '', '', '']
let total = 0
for (let i = 0; i < n; i = i + 1) {
  const s = prefix + tails[i & 1]
  ring[i & 7] = s
  total = total + s.charCodeAt(i & 15)
}
console.log(total)
console.log(ring[(n - 1) & 7])
