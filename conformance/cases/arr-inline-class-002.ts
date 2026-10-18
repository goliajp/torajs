// Arrays holding their class in place through every growth step: the
// first push into an empty array, the pushes that outgrow the space
// inside the array itself and move the elements out to a buffer, and
// the later regrowths of that buffer. Three fields (one boolean) make
// an element three words wide; two fields make it two.

class Rec {
  a: number
  b: number
  on: boolean
  constructor(a: number, b: number, on: boolean) {
    this.a = a
    this.b = b
    this.on = on
  }
}

class Pair {
  x: number
  y: number
  constructor(x: number, y: number) {
    this.x = x
    this.y = y
  }
}

function fill(n: number): Rec[] {
  const rs: Rec[] = []
  let i = 0
  while (i < n) {
    rs.push(new Rec(i, i * 0.5, i % 3 === 0))
    i = i + 1
  }
  return rs
}

function check(rs: Rec[]): string {
  let sa = 0
  let sb = 0
  let on = 0
  for (let i = 0; i < rs.length; i = i + 1) {
    sa = sa + rs[i].a
    sb = sb + rs[i].b
    if (rs[i].on) on = on + 1
  }
  return rs.length + " " + sa + " " + sb + " " + on
}

for (const n of [0, 1, 2, 3, 4, 5, 7, 8, 9, 16, 17, 100, 1000]) {
  console.log(n, check(fill(n)))
}

const ps: Pair[] = []
console.log(ps.length)
console.log(ps.push(new Pair(1, -1)))
console.log(ps[0].x, ps[0].y)
let total = 0
let i = 0
while (i < 300) {
  ps.push(new Pair(i, i + 0.25))
  total = total + ps[ps.length - 1].y - ps[0].y
  i = i + 1
}
console.log(ps.length, total, ps[150].x, ps[299].y, ps[300].x)
