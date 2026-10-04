// A constructor that throws for some inputs, inside a loop whose
// other iterations build and release the object normally.
class Checked {
  n: number
  m: number
  constructor(n: number) {
    if (n % 7 === 3) throw new RangeError("bad " + n)
    this.n = n
    this.m = n * 2
  }
  total(): number {
    return this.n + this.m
  }
}

let ok = 0
let bad = 0
let msg = ""
for (let i = 0; i < 50; i = i + 1) {
  try {
    const c = new Checked(i)
    ok = ok + c.total()
  } catch (e) {
    bad = bad + 1
    if (e instanceof RangeError) msg = e.message
  }
}
console.log("ctor", ok, bad, msg)

function make(i: number): number {
  const c = new Checked(i)
  return c.total()
}
let sum = 0
for (let i = 0; i < 20; i = i + 1) {
  if (i % 7 !== 3) sum = sum + make(i)
}
try {
  make(3)
} catch (e) {
  console.log("outer", e instanceof RangeError, sum)
}
