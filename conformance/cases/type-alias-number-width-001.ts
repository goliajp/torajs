// A type alias names its target (TS §3.10): a slot annotated with an
// alias of `number` holds the same f64 values a `: number` slot does,
// so none of them may be narrowed to an integer.

type N = number;
type M = N;
type U = number | undefined;
type V = M | undefined;
type Row = N[];

const a: number[] = [1.5, 2.5];
console.log(a.map((x: N) => x * 2));
console.log(a.map((x: M) => x * 2), a.filter((x: M) => x > 2));
console.log(a.map((x: U) => (x ?? 0) * 3), a.map((x: V) => (x ?? 1) / 2));

let s: N = 0;
for (const v of a) {
  const w: M = v;
  s += w;
}
console.log(s);

function half(x: M): M {
  return x / 2;
}
console.log(half(3), half(0.5));

let t: M = 1;
t = t / 4;
console.log(t);

const b: Row = [1, 2];
b.push(0.5);
const c: M[] = b.map((x: N) => x + 0.25);
console.log(b, c);

type P = { x: N; y: M };
const p: P = { x: 1, y: 2 };
p.x = p.x / 8;
console.log(p.x, p.y + 0.5);

class Acc {
  total: N = 0;
  add(v: M): void {
    this.total += v;
  }
}
const acc = new Acc();
acc.add(0.75);
acc.add(1);
console.log(acc.total);
