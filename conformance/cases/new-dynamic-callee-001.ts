// ES §13.3 — the callee of `new` is a MemberExpression, and the
// constructor it names is only known once that expression has been
// evaluated.
class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

const registry: any = { Point: Point };
const p = new registry.Point(3, 4);
console.log(p.x + p.y);

const table: any = [Point];
const q = new table[0](1, 2);
console.log(q.x * q.y);

class Holder {
  Inner = class {
    label(): string {
      return "inner";
    }
  };
}
const h: any = new Holder();
const inner = new h.Inner();
console.log(inner.label());
