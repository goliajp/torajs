class Vec2 {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  norm(): number {
    return this.x * this.x + this.y * this.y;
  }
  scale(k: number): Vec2 {
    return new Vec2(this.x * k, this.y * k);
  }
}

let total: number = 0;
let n: number = 100000;
for (let i: number = 0; i < n; i = i + 1) {
  let v: Vec2 = new Vec2(i, i + 1);
  let s: Vec2 = v.scale(2);
  total = total + s.norm();
}
console.log(total);
