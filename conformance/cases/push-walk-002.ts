// An arm that pushes without moving the counter: the trips outnumber
// the characters, and every append past the room the buffer had at
// entry still lands.
class Pair {
  a: number
  b: number
  constructor(a: number, b: number) {
    this.a = a
    this.b = b
  }
}
function doubled(s: string): number[] {
  const out: number[] = []
  let hold = 0
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (hold === 1) {
      hold = 0
      out.push(-c)
      i = i + 1
    } else if (c === 97) {
      hold = 1
      out.push(c)
    } else {
      out.push(c)
      i = i + 1
    }
  }
  return out
}
function pairs(s: string): Pair[] {
  const out: Pair[] = []
  let left = 3
  let i = 0
  while (i < s.length) {
    if (left > 0) {
      left = left - 1
      out.push(new Pair(i, left))
    } else {
      left = 3
      out.push(new Pair(-i, s.charCodeAt(i)))
      i = i + 1
    }
  }
  return out
}
const d = doubled('abaca')
console.log(d.length, d.join(','))
const many = doubled('a'.repeat(1000))
let sum = 0
for (let k = 0; k < many.length; k = k + 1) sum = sum + many[k]
console.log(many.length, sum)
const p = pairs('xyz')
let t = ''
for (let k = 0; k < p.length; k = k + 1) t = t + p[k].a + '/' + p[k].b + ' '
console.log(p.length, t)
const q = pairs('q'.repeat(500))
console.log(q.length, q[q.length - 1].a, q[q.length - 1].b)
