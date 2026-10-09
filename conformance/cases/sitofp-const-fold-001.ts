// an inlined method whose number parameter receives an integer literal:
// the literal's float form must be the value the runtime conversion
// gives, and the loops it feeds must keep f64 results bit for bit
class Vec2 {
  x: number
  y: number
  constructor(x: number, y: number) {
    this.x = x
    this.y = y
  }
  norm(): number {
    return this.x * this.x + this.y * this.y
  }
  scale(k: number): Vec2 {
    return new Vec2(this.x * k, this.y * k)
  }
}

// a trip count read at run time leaves the sums unbounded at compile
// time, so they demote behind a per-iteration range guard
const n = parseInt("5000", 10)
let total = 0
for (let i = 0; i < n; i = i + 1) {
  const v = new Vec2(i & 1023, (i & 1023) + 1)
  const s = v.scale(2)
  total = total + s.norm()
}
console.log(total)

// the running sum crosses 2^53, where f64 addition starts rounding
let big = 9007199254000000
for (let i = 0; i < n; i = i + 1) {
  const v = new Vec2(i & 1023, (i & 1023) + 1)
  big = big + v.scale(3).norm()
}
console.log(big)

let neg = 0
for (let i = 0; i < 300; i = i + 1) {
  const v = new Vec2(i % 7, -(i % 5))
  const s = v.scale(-3)
  neg = neg + s.x - s.y
}
console.log(neg)

const z = new Vec2(0, 1).scale(-3)
console.log(1 / z.x, z.y)

const w = new Vec2(1, 3).scale(9007199254740993)
console.log(w.x, w.y, w.norm())
