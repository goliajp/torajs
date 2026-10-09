// Method calls on the elements of a `(T | undefined)[]` literal, which
// lives on the any lane: a `for-of` binding over it holds each element
// as an `any`, so its methods are looked up on the value itself.
interface Shape {
  area(): number;
  name(): string;
}
class Sq implements Shape {
  s: number;
  constructor(s: number) {
    this.s = s;
  }
  area() {
    return this.s * this.s;
  }
  name() {
    return "sq" + this.s;
  }
}
const ms: (Shape | undefined)[] = [new Sq(2), undefined, new Sq(3)];
for (const m of ms) console.log(m?.area(), m?.name());
let total = 0;
const names: string[] = [];
for (const m of ms) {
  if (m !== undefined) {
    total += m.area();
    names.push(m.name());
    const a: number = m.area();
    console.log(a > 3, a * 2, m.name().length);
  }
}
console.log(total, names.join(","));
function sum(xs: (Shape | undefined)[]): number {
  let t = 0;
  for (const x of xs) t += x?.area() ?? 0;
  return t;
}
console.log(sum(ms));

type P = { f(k: number): number; v: number };
const ps: (P | undefined)[] = [{ f: (k: number) => k + 1, v: 5 }, undefined];
for (const p of ps) console.log(p?.f(1), p?.v);
for (const p of ps) if (p !== undefined) console.log(p.f(10) + 1, p.v + 1);
