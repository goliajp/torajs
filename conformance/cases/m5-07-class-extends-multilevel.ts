class Shape {
  name: string;
  constructor(n: string) { this.name = n; }
  describe(): void { console.log(this.name); }
}
class Polygon extends Shape {
  sides: number;
  constructor(n: string, s: number) {
    super(n);
    this.sides = s;
  }
  count(): void { console.log(this.sides); }
}
class Triangle extends Polygon {
  base: number;
  constructor(b: number) {
    super("triangle", 3);
    this.base = b;
  }
  area(h: number): number { return this.base * h; }
}
let t = new Triangle(10);
t.describe();
t.count();
console.log(t.area(8));
