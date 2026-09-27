// a pop loop keeps the array's length in a register; everything that
// can observe the length from outside the loop has to see it current

// plain drain: the length after the loop
const a: number[] = []
for (let i = 0; i < 10; i = i + 1) a.push(i)
let s = 0
while (a.length > 0) s = s + a.pop()!
console.log(s, a.length)

// break out halfway
const b: number[] = []
for (let i = 0; i < 10; i = i + 1) b.push(i * 2)
let t = 0
while (b.length > 0) {
  const x = b.pop()!
  if (x < 10) break
  t = t + x
}
console.log(t, b.length, b[b.length - 1])

// an unrelated call inside the loop
const c: number[] = []
for (let i = 0; i < 5; i = i + 1) c.push(i)
while (c.length > 2) {
  console.log("pop", c.pop()!)
}
console.log(c.length, c.join(","))

// a frozen array: the guard throws on the first pop, the length stays
const d: number[] = [1, 2, 3]
Object.freeze(d)
try {
  while (d.length > 0) d.pop()
} catch (e) {
  console.log("caught", e instanceof TypeError, d.length)
}

// frozen partway: the loop throws after draining some
const e2: number[] = [1, 2, 3, 4, 5]
let n = 0
try {
  while (e2.length > 0) {
    e2.pop()
    n = n + 1
    if (n === 2) Object.freeze(e2)
  }
} catch (err) {
  console.log("partial", n, e2.length, e2.join(","))
}

// nested: the inner loop drains, the outer refills
const f: number[] = []
let u = 0
for (let r = 0; r < 3; r = r + 1) {
  for (let i = 0; i <= r; i = i + 1) f.push(i + r)
  while (f.length > 0) u = u + f.pop()!
  console.log("round", r, u, f.length)
}

// a function that reads the length through the array
function len(xs: number[]): number {
  return xs.length
}
const g: number[] = [4, 5, 6, 7]
let w = 0
while (g.length > 1) {
  g.pop()
  w = w + len(g)
}
console.log(w, g.length)
