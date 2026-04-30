class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  distSq(): number {
    return this.x * this.x + this.y * this.y;
  }
  shift(dx: number, dy: number): void {
    this.x = this.x + dx;
    this.y = this.y + dy;
  }
}
let p = new Point(3, 4);
console.log(p.distSq());
p.shift(1, 2);
console.log(p.x);
console.log(p.y);
console.log(p.distSq());
