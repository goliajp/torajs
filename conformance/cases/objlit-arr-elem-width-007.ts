// The fractional write lands on an array element itself (through an
// `any` view, or directly), and the class form of the same layout.
type P = { x: number; y: number };
const pts: P[] = [{ x: 1, y: 2 }, { x: 3, y: 4 }];
(pts[0] as any).x = 5.5;
console.log(pts[0].x, pts[1].x);
const qs: P[] = [{ x: 1, y: 2 }, { x: 3, y: 4 }];
qs[1].x = 0.5;
console.log(qs[0].x, qs[1].x);
const plain: P = { x: 2, y: 3 };
console.log(plain.x);
class C {
  constructor(public x: number, public y: number) {}
}
const cs: C[] = [new C(1, 2), new C(3, 4)];
console.log(cs[0].x, cs[1].x * 2);
const c = new C(1, 2);
(c as any).x = 5.5;
console.log(c.x, cs.map((k) => k.x).join(","));
const mixed = [{ x: 1, y: 2 }, { x: 2.5, y: 3 }];
console.log(mixed[0].x, mixed[1].x);
const a = { x: 9, y: 0 };
const b = { x: 1.5, y: 0 };
console.log(a.x, b.x, [a, b].map((o) => o.x).join(","));
