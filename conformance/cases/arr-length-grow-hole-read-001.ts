// growing a number[] through `length` leaves holes; an in-bounds read
// of one answers undefined, not 0

let b: number[] = [1, 2, 3]
b.length = 5
let t = ""
for (let i = 0; i < b.length; i++) {
  t = t + b[i] + ","
}
console.log(t)
console.log(b[3] === undefined, typeof b[4])

function grow(xs: number[], n: number): number {
  xs.length = n
  let holes = 0
  for (let i = 0; i < xs.length; i++) {
    if (xs[i] === undefined) {
      holes++
    }
  }
  return holes
}
console.log(grow([7, 8], 6))

let c: number[] = [4, 5, 6]
c.length = 0
c.push(9)
console.log(c.join(","), c.length)

let d: number[] = [1]
d.length += 2
console.log(d[1], d.length)
