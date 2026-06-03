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

let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const v = new Vec2(i, i + 1)
  const s = v.scale(2)
  total = total + s.norm()
}
console.log(total)
