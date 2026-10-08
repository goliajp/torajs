// An interface a class instance flows into, next to a local binding
// that only ever holds the literal it was declared with: the binding's
// fields are read and written in place.
interface Pt {
  x: number;
  y: number;
}
class Tagged {
  tag = 1;
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}
function norm(p: Pt): number {
  return p.x * p.x + p.y * p.y;
}
function localSum(n: number): number {
  let t = 0;
  for (let i = 0; i < n; i++) {
    const q: Pt = { x: i, y: 2 };
    q.y = q.y + 1;
    t += q.x + q.y;
  }
  return t;
}
function shadowed(): number {
  const q: Pt = { x: 1, y: 1 };
  let s = q.x;
  {
    const q: Pt = new Tagged(5, 6);
    s += q.x + q.y;
  }
  return s + q.y;
}
console.log(norm(new Tagged(3, 4)), norm({ x: 1, y: 2 }), localSum(5), shadowed());
