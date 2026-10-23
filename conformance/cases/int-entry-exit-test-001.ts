// A loop whose counter only counts compares it with an f64 bound nothing
// is known of; the bound may be fractional, negative, NaN, infinite or
// past 2^53, and every comparison operator must keep its f64 answer.
function bound(s: string): number {
  return parseFloat(s)
}

function count(label: string, s: string, cap: number): void {
  const n = bound(s)
  let c = 0
  for (let i = 0; i < n; i = i + 1) {
    c = c + 1
    if (c >= cap) break
  }
  let d = 0
  for (let i = 0; i <= n; i = i + 1) {
    d = d + 1
    if (d >= cap) break
  }
  let e = 0
  for (let i = 0; i > -n; i = i - 1) {
    e = e + 1
    if (e >= cap) break
  }
  let f = 0
  for (let i = 0; i >= -n; i = i - 1) {
    f = f + 1
    if (f >= cap) break
  }
  console.log(label, c, d, e, f)
}

const inputs = ['0', '1', '2.5', '3', '-1', '-0.5', 'NaN', 'Infinity', '-Infinity', '4503599627370497', '9007199254740993', '1e300', '-1e300']
for (const s of inputs) count(s, s, 7)

// the counter handed to a call and masked: the test only counts
const n = bound('5.000001')
let out = ''
for (let i = 0; i < n; i = i + 1) {
  out = out + String(i & 3)
}
console.log(out)
