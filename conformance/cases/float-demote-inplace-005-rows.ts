// The sum restarts on every outer trip; only one trip crosses 2^53,
// and the trip after it starts back as an integer.
const n = parseInt(process.argv.length > 99 ? '3' : '4000', 10)
const tail = String(process.argv.length > 99 ? 'zz' : 'xy')
const prefix = '  Hello42 word  '
const ring: string[] = ['', '', '', '', '', '', '', '']
let out = ''
for (let r = 0; r < 4; r = r + 1) {
  let sum = r === 2 ? 9007199254740991 - 250000 : 7
  for (let i = 0; i < n; i = i + 1) {
    const s = prefix + tail
    ring[i & 7] = s
    sum = sum + s.charCodeAt(i & 15)
  }
  out = out + ' ' + sum
}
console.log(out)
