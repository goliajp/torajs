// A program that fills one size class, lets all of it go, then fills
// another: the first phase's spans empty out and their pages carry
// the second phase, so the process does not hold both phases at once.
// The answers only check that every cell was written and freed in
// order; the memory behaviour is read off the AOT binary's max RSS.

class Wide {
  a: number
  b: number
  c: number
  d: number
  constructor(i: number) {
    this.a = i
    this.b = i + 1
    this.c = i + 2
    this.d = i + 3
  }
}

class Narrow {}

const n = 1000000
let wide: Wide[] = []
for (let i = 0; i < n; i = i + 1) {
  wide.push(new Wide(i))
}
let sum = 0
for (let i = 0; i < n; i = i + 1) {
  sum = sum + wide[i].d - wide[i].a
}
console.log(sum)
wide = []

let narrow: Narrow[] = []
for (let i = 0; i < n; i = i + 1) {
  narrow.push(new Narrow())
}
console.log(narrow.length)
console.log("done")
