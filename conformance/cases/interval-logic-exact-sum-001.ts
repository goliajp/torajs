// A digit loop whose accumulated value crosses 2^53: every step must
// give the f64 the spec gives, including an integer compare on the
// result, whether the loop ran as integers or not.

function parse(src: string): number {
  let v = 0
  let i = 0
  while (i < src.length) {
    const d = src.charCodeAt(i)
    if (d < 48 || d > 57) break
    v = v * 10 + (d - 48)
    i = i + 1
  }
  return v
}

const a = parse("9007199254740993")
console.log(a, a === 9007199254740992, a === 9007199254740993, a + 1)
const b = parse("9007199254740999")
console.log(b, b === 9007199254741000, b - 9007199254740992)
const c = parse("90071992547409929")
console.log(c, c === 90071992547409920, c / 10)
const e = parse("18014398509481985")
console.log(e, e === 18014398509481984)
let total = 0
const parts = ["9007199254740991", "9007199254740992", "9007199254740993", "1", "9"]
for (let k = 0; k < parts.length; k = k + 1) total = total + parse(parts[k])
console.log(total, total === 27021597764222980)
