// An integral object literal stored in a narrow declared slot keeps the
// slot's integer width even when an unrelated same-shaped object
// elsewhere takes a fractional value: the literal's layout follows its
// own slot, not every literal of the same shape.
type P = { x: number; y: number };
const pts: P[] = [{ x: 1, y: 2 }, { x: 3, y: 4 }];
console.log(pts[0].x, pts[1].x * 2);
const doubled: P[] = pts.map((p) => ({ x: p.x * 2, y: p.y }));
console.log(doubled[0].x, doubled[1].x);
let s = 0;
for (const p of pts) s += p.x;
console.log(s);
pts[1] = { x: 7, y: 8 };
pts.push({ x: 9, y: 10 });
console.log(pts[1].x, pts[2].x, pts.length);
const grid: P[][] = [[{ x: 1, y: 2 }], [{ x: 3, y: 4 }]];
console.log(grid[0][0].x, grid[1][0].x);
function mk(): P[] {
  return [{ x: 5, y: 6 }];
}
console.log(mk()[0].x);
function sum(ps: P[]): number {
  let t = 0;
  for (const p of ps) t += p.x;
  return t;
}
console.log(sum([{ x: 1, y: 2 }, { x: 3, y: 4 }]));
type H = { ps: P[] };
const h: H = { ps: [{ x: 11, y: 2 }] };
console.log(h.ps[0].x);
let later: P[] = [];
later = [{ x: 4, y: 2 }];
console.log(later[0].x);
const q: P = { x: 1, y: 2 };
(q as any).x = 5.5;
console.log(q.x);
const r: P = { x: 1, y: 2 };
r.x = 0.25;
console.log(r.x, pts[0].x);
