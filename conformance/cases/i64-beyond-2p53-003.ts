// A counter that starts near 2^53: a small step does not keep it exact,
// because where it starts counts as much as how far it moves.

let s = 9007199254740000
let i = 0
while (i < 1000) {
  s += 2
  i++
}
console.log(s)

let t = 9007199254740990
for (let k = 0; k < 4; k++) {
  t = t + 1
  console.log(t)
}

let u = 9007199254740990
for (let k = 0; k < 4; k++) u++
console.log(u)

let d = -9007199254740990
for (let k = 0; k < 4; k++) d -= 1
console.log(d)

function walk(start: number, n: number): number {
  let v = start
  for (let k = 0; k < n; k++) v += 3
  return v
}
console.log(walk(9007199254740980, 10), walk(5, 10))

class Tally {
  n: number = 9007199254740990
  bump(): void {
    this.n = this.n + 1
  }
}
const tl = new Tally()
tl.bump()
tl.bump()
tl.bump()
console.log(tl.n)

let cnt = 9007199254740990
const inc = () => {
  cnt = cnt + 1
}
inc()
inc()
inc()
console.log(cnt)
