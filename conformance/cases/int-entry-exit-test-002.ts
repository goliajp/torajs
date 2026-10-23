// A loop whose counter is compared with an f64 bound nothing is known of
// and also read as an f64 elsewhere in the body: pushed, kept in an
// object field that may be absent, summed with a value read back from
// an array. The comparison keeps its f64 answer for fractional,
// negative, NaN, infinite and huge bounds.
function bound(s: string): number {
  return parseFloat(s)
}

type Opt = { a?: number; b: number }

function shared(label: string, s: string, cap: number): void {
  const n = bound(s)
  const seen: number[] = []
  for (let i = 0; i < n; i = i + 1) {
    seen.push(i)
    if (seen.length >= cap) break
  }
  let total = 0
  let made = 0
  for (let i = 0; i <= n; i = i + 1) {
    const o: Opt = i % 3 === 0 ? { a: i, b: i } : { b: i }
    total = total + (o.a ?? 0) + o.b
    made = made + 1
    if (made >= cap) break
  }
  let mixed = 0
  for (let i = 0; i > -n; i = i - 1) {
    mixed = mixed + seen[(-i) % (seen.length || 1)] * 0 + i
    if (mixed <= -100) break
  }
  console.log(label, seen.join(','), total, made, mixed)
}

const inputs = ['0', '1', '2.5', '3', '-1', '-0.5', 'NaN', 'Infinity', '-Infinity', '4503599627370497', '9007199254740993', '1e300']
for (const s of inputs) shared(s, s, 6)

// an inner loop under an outer one, both against bounds nothing is
// known of: the inner test runs every outer round
const rounds = bound('3.5')
const pieces = bound('4.25')
let acc = 0
for (let r = 0; r < rounds; r = r + 1) {
  let s = ''
  for (let j = 0; j < pieces; j = j + 1) {
    s = s + 'ab'
  }
  acc = acc + s.length
}
console.log(acc)

// at the top level: the counter's image is pushed as well as compared
const b1 = bound('3.5')
const q: number[] = []
for (let i = 0; i < b1; i = i + 1) {
  q.push(i)
}
const b2 = bound('NaN')
for (let i = 0; i < b2; i = i + 1) {
  q.push(i)
}
const b3 = bound('2')
for (let i = 0; i <= b3; i = i + 1) {
  q.push(i * 10)
}
const b4 = bound('-2.5')
for (let i = 0; i > b4; i = i - 1) {
  q.push(i)
}
console.log(q.join(','))

// the image kept in a field that may be absent, the sum taking a hole
function make(i: number): Opt {
  if (i % 4 === 0) return { a: i, b: i }
  return { b: i }
}
const b5 = bound('9.5')
let sum = 0
let created = 0
for (let i = 0; i < b5; i = i + 1) {
  const o = make(i)
  if (!('a' in o)) {
    o.a = 2
    created = created + 1
  }
  o.b = o.b + 1
  sum = sum + (o.a ?? 0) + o.b
}
console.log(sum, created)
