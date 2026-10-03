// `lit + n` and `n.toString()` over every digit count, both signs,
// zero and the safe-integer edges, with Latin-1 and UTF-16 prefixes
const n = parseInt(process.argv[2] ?? '19', 10)
let p = 1
for (let k = 0; k < n; k = k + 1) {
  const lo = p - 1
  const hi = p
  console.log('a' + lo, 'b' + hi, 'c' + -lo, 'd' + -hi, lo.toString(), (-hi).toString())
  console.log('ключ' + hi, 'é' + -lo, (hi + 1).toString().length)
  p = p * 10
}
let sum = 0
for (let i = -1005; i < 1005; i = i + 1) {
  const s = 'v' + i
  sum = sum + s.length + s.charCodeAt(s.length - 1)
}
console.log(sum)
const edges = [0, -0, 9007199254740991, -9007199254740991, 4294967295, -4294967296, 2147483647, -2147483648]
for (const e of edges) {
  console.log('e' + e, e.toString(), 'ж' + e)
}
let big = 9007199254740000
for (let i = 0; i < 3; i = i + 1) {
  console.log('big' + (big + i), String(big - i))
}
// integer-lane counters straddling digit-count changes
let acc = ''
for (let i = 7; i < 12; i = i + 1) acc = acc + ('i' + i) + i.toString()
for (let i = 9995; i < 10005; i = i + 1) acc = acc + ('i' + i) + i.toString()
for (let i = 99999995; i < 100000005; i = i + 1) acc = acc + ('i' + i)
for (let i = 999999999995; i < 1000000000005; i = i + 1) acc = acc + ('i' + i)
for (let i = 999999999999995; i < 1000000000000005; i = i + 1) acc = acc + ('i' + i)
for (let i = 9007199254740985; i < 9007199254740992; i = i + 1) acc = acc + ('m' + i) + i.toString()
for (let i = -9007199254740991; i < -9007199254740985; i = i + 1) acc = acc + ('n' + i) + i.toString()
for (let i = -12; i < -7; i = i + 1) acc = acc + ('ю' + i)
console.log(acc)
