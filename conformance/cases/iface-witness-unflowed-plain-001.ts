// An object type that an unrelated class happens to satisfy, while no
// instance of that class is ever used as the type: its values are
// literals, read at their own field offsets.
type Vec = { x: number; y: number };
class Particle {
  id: number;
  x: number;
  y: number;
  constructor(i: number) {
    this.id = i;
    this.x = i;
    this.y = i * 2;
  }
}
function len2(v: Vec): number {
  return v.x * v.x + v.y * v.y;
}
let t = 0;
for (let i = 0; i < 10; i++) {
  const v: Vec = { x: i, y: i + 1 };
  t += len2(v);
}
const p = new Particle(3);
console.log(t, p.id + p.x + p.y);
