// Adapted from test262: two unrelated classes coexisting in the same
// module + instances of each used in the same expressions.
class Pt {
  x: number;
  y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  sum(): number { return this.x + this.y; }
}

class Box {
  w: number;
  h: number;
  constructor(w: number, h: number) { this.w = w; this.h = h; }
  area(): number { return this.w * this.h; }
}

function check(): number {
  let p = new Pt(3, 4);
  let b = new Box(5, 6);
  if (p.sum() !== 7) { throw "#1"; }
  if (b.area() !== 30) { throw "#2"; }
  if (p.sum() + b.area() !== 37) { throw "#3"; }
  return 0;
}
console.log(check());
