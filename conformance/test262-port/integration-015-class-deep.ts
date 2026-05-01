// Integration: multi-class composition with extends + super(args).
// Exercises M-OO.1 + M-OO.2 (single class, single inheritance,
// super arg passthrough). Method names are unique per class to
// avoid the v0 no-override constraint.
class Shape {
  name: string;
  constructor(n: string) {
    this.name = n;
  }
  describe_shape(): string {
    return "shape:" + this.name;
  }
}

class Circle extends Shape {
  radius: number;
  constructor(r: number) {
    super("circle");
    this.radius = r;
  }
  area(): f64 {
    return Math.PI * this.radius * this.radius;
  }
  perimeter(): f64 {
    return 2 * Math.PI * this.radius;
  }
}

class Rect extends Shape {
  w: number;
  h: number;
  constructor(w: number, h: number) {
    super("rect");
    this.w = w;
    this.h = h;
  }
  area_rect(): number {
    return this.w * this.h;
  }
  perimeter_rect(): number {
    return 2 * (this.w + this.h);
  }
}

function check(): number {
  let c = new Circle(5);
  if (c.name !== "circle") { throw "#1: super passthrough"; }
  if (c.radius !== 5) { throw "#2"; }
  if (c.describe_shape() !== "shape:circle") { throw "#3: parent method"; }

  // area = pi * 25 ≈ 78.54; trunc to int.
  let area_int = Math.floor(c.area());
  if (area_int !== 78) { throw "#4: area floor"; }

  let r = new Rect(3, 4);
  if (r.name !== "rect") { throw "#5"; }
  if (r.area_rect() !== 12) { throw "#6"; }
  if (r.perimeter_rect() !== 14) { throw "#7"; }
  if (r.describe_shape() !== "shape:rect") { throw "#8: inherited"; }

  // Multiple instances.
  let c2 = new Circle(10);
  if (c2.radius !== 10) { throw "#9"; }
  if (c.radius !== 5) { throw "#10: instance independence"; }
  return 0;
}
console.log(check());
