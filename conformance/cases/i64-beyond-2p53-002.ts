// Sums of integers past 2^53 round at every step: `z + z + z` is not
// the exact triple, and `p + 2` lands back on an even neighbour.

function sum3(z: number): number {
  return z + z + z
}

let z = 9007199254740991
console.log(z + z + z, sum3(z), sum3(3002399751580331))

let p = 9007199254740991
const q = p + 2
console.log(q, q - p, p + 1 + 1, p + 2)

let a = 4503599627370497
let b = a + a
let c = b + a
console.log(b, c, c - b)

const xs: number[] = [9007199254740990, 9007199254740991]
console.log(xs[0] + xs[1], xs[1] + 3 - 3)

let r = 1152921504606846976
console.log(r + 1, r + 1 === r)
