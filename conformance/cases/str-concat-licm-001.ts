// A concat whose operands do not change in the loop is built once
// and shared: every path out of the loop must release it exactly once,
// and a loop that never runs must not build it.
const prefix = '  Hello42 word  '
const tail = String(process.argv.length > 99 ? 'z' : 'x')
const ring: string[] = ['', '', '', '', '', '', '', '']
let total = 0

// the escaping twin of the bench case: stored, read, overwritten
for (let i = 0; i < 1000; i = i + 1) {
  const s = prefix + tail
  ring[i & 7] = s
  total = total + s.charCodeAt(i & 15)
}
console.log(total, ring[7], ring[0] === ring[7])

// zero-trip loop: nothing is built
let n0 = 0
for (let i = 0; i < 0; i = i + 1) {
  const s = prefix + tail
  n0 = n0 + s.length
}
console.log(n0)

// break out of the loop while the binding is still live
let seen = ''
for (let i = 0; i < 100; i = i + 1) {
  const s = tail + prefix
  if (i === 3) {
    seen = s
    break
  }
  ring[i] = s
}
console.log(seen, ring.length, ring[2] === seen)

// the store grows the array, so the out-of-range arm runs too
const grown: string[] = []
for (let i = 0; i < 20; i = i + 1) {
  const s = prefix + tail
  grown[i] = s
}
console.log(grown.length, grown[19])

// chained invariant concats, one read by the next
let last = ''
for (let i = 0; i < 10; i = i + 1) {
  const a = prefix + tail
  const b = a + tail
  ring[i & 7] = b
  last = b
}
console.log(last, ring[1] === last)

// nested loops: the inner concat is invariant in both
let count = 0
for (let i = 0; i < 5; i = i + 1) {
  for (let j = 0; j < 5; j = j + 1) {
    const s = tail + tail
    ring[j] = s
    count = count + s.length
  }
}
console.log(count, ring[4])

// continue skips the store on odd rounds
let odd = 0
for (let i = 0; i < 50; i = i + 1) {
  const s = prefix + tail
  if (i % 2 === 1) {
    odd = odd + s.length
    continue
  }
  ring[i & 7] = s
}
console.log(odd, ring[6])

// the binding changes in the loop: not invariant
let acc = tail
for (let i = 0; i < 5; i = i + 1) {
  const s = prefix + acc
  acc = acc + 'y'
  ring[i] = s
}
console.log(ring[4], acc)
