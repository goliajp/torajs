// `(x = e) !== null` and `while (x = e)` narrow `x` like `x !== null`.
const re = /(\d)/g
const all: string[][] = []
let m: string[] | null
while ((m = re.exec('a1b2c3')) !== null) all.push(m)
console.log(all.length, all.map((x) => x[1]).join(''))
let k: string[] | null
if ((k = 'x9'.match(/\d/)) !== null) console.log(k[0], k.length)
if (null === (k = 'xy'.match(/\d/))) console.log('miss', k)
const q: string[] = ['a', 'bb', 'ccc']
let s: string | null
let n = 0
function take(): string | null {
  return n < q.length ? q[n++] : null
}
while ((s = take())) console.log(s.length, s.toUpperCase())
