// Adapted from test262: subclass ctor passes computed args to super().
class Vec2 {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  sum(): number { return this.x + this.y; }
}

class ScaledVec2 extends Vec2 {
  scale: number;
  constructor(x: number, y: number, k: number) {
    super(x * k, y * k);
    this.scale = k;
  }
  product(): number { return this.x * this.y; }
}

function check(): number {
  let v = new ScaledVec2(3, 4, 2);
  if (v.x !== 6) { throw "#1"; }
  if (v.y !== 8) { throw "#2"; }
  if (v.scale !== 2) { throw "#3"; }
  if (v.sum() !== 14) { throw "#4"; }
  if (v.product() !== 48) { throw "#5"; }
  return 0;
}
console.log(check());
