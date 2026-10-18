// Fill loops the compiler reserves capacity ahead of, over an array
// holding its class in place: a `for` and a `while` loop, one append
// a trip and two, onto an empty array and onto one already holding
// elements, and a loop that runs zero times.

class Pt {
  x: number
  y: number
  constructor(x: number, y: number) {
    this.x = x
    this.y = y
  }
}

function sum(ps: Pt[]): number {
  let s = 0
  for (let i = 0; i < ps.length; i = i + 1) {
    s = s + ps[i].x * 3 + ps[i].y
  }
  return s
}

function viaFor(n: number): Pt[] {
  const ps: Pt[] = []
  for (let i = 0; i < n; i = i + 1) ps.push(new Pt(i, i * 2))
  return ps
}

function viaForTwice(n: number): Pt[] {
  const ps: Pt[] = []
  ps.push(new Pt(-1, -1))
  for (let i = 0; i < n; i = i + 1) {
    ps.push(new Pt(i, 1))
    ps.push(new Pt(1, i))
  }
  return ps
}

function viaWhile(n: number): Pt[] {
  const ps: Pt[] = []
  let i = 0
  while (i < n) {
    ps.push(new Pt(i, 0.5))
    i = i + 1
  }
  return ps
}

for (const n of [0, 1, 3, 9, 64, 1000]) {
  const a = viaFor(n)
  const b = viaForTwice(n)
  const c = viaWhile(n)
  console.log(n, a.length, sum(a), b.length, sum(b), c.length, sum(c))
}

// a scalar array with two appends a trip
function evens(n: number): number[] {
  const xs: number[] = []
  for (let i = 0; i < n; i = i + 1) {
    xs.push(i)
    xs.push(i * 2)
  }
  return xs
}
const ev = evens(500)
console.log(ev.length, ev[0], ev[999], ev[998])
