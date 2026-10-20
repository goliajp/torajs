// A walk whose counter steps by other than one: by two and three, by a
// half, and back to the start. The appends outrun the room the buffer
// had at entry, which grows on the way, also on a receiver that a
// shift left with a deque head. (A shift anywhere keeps a class's
// arrays from holding it in place; push-walk-002 grows those.)
function strides(s: string): number[] {
  const out: number[] = []
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (c === 32) {
      i = i + 3
    } else {
      out.push(c)
      out.push(i)
      i = i + 2
    }
  }
  return out
}
function halves(s: string, out: number[]): number[] {
  let i = 0
  while (i < s.length) {
    if (i > 2) {
      out.push(i)
    }
    i = i + 0.5
  }
  return out
}
function rewinds(s: string): number[] {
  const out: number[] = []
  let turns = 0
  let i = 0
  while (i < s.length) {
    out.push(i + 0.5)
    out.push(-turns)
    if (turns < 300 && i === 5) {
      turns = turns + 1
      i = 0
    } else {
      i = i + 1
    }
  }
  return out
}
function sum(xs: number[]): number {
  let t = 0
  for (let k = 0; k < xs.length; k = k + 1) t = t + xs[k]
  return t
}
const a = strides('ab  cdefg hij')
console.log(a.length, a.join(','))
const b = strides('z'.repeat(999))
console.log(b.length, sum(b))
const e: number[] = []
console.log(halves('abcdef', e).join(','))
const h: number[] = [10, 11, 12, 13, 14, 15, 16, 17]
h.shift()
h.shift()
h.shift()
const g = halves('x'.repeat(700), h)
console.log(g.length, g[0], g[4], g[g.length - 1], sum(g))
const r = rewinds('abcdefgh')
console.log(r.length, r[r.length - 2], r[r.length - 1], r[1000], r[1001], sum(r))
const r2 = rewinds('abcd')
console.log(r2.length, r2.join(','))
