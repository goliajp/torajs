// A function value of another face stored into a property slot: an
// object literal's field against a declared struct type, a class field,
// a later field store. The call through the field answers what the
// stored value returns, a string included.
const mk2 = (k: number) => (y?: number) => (y === undefined ? 5 : y) + k;
const mks = (k: number) => (y?: number): any => "s" + k;
type T = (y?: number) => number;

const o: { f: T } = { f: mk2(1) };
console.log(o.f(), o.f(2));

class C {
  f: T = mk2(1);
  g: T = (y?: number) => 40;
  run(): number {
    return this.f() + this.g();
  }
}
class D extends C {
  h = 3;
}
const c = new C();
console.log(c.f(), c.f(3), c.run());
const d = new D();
const cd: C = d;
console.log(d.f(), d.g(), d.run(), cd.f(2));

// one shape, a stored arrow and a method: the method keeps its receiver
const a: { f: T; x: number } = { f: mk2(10), x: 7 };
const b: { f: T; x: number } = {
  x: 9,
  f(y?: number) {
    return this.x + (y ?? 0);
  },
};
console.log(a.f(), b.f(), b.f(1));

const later: { f: T } = { f: (y?: number) => 1 };
console.log(later.f());
later.f = mk2(100);
console.log(later.f(), later.f(1));

const s: { f: T } = { f: mks(7) };
console.log(s.f(), typeof s.f());

const p: { f?: T } = { f: mk2(2) };
if (p.f) console.log(p.f());

function inner(): number {
  const q: { f: T } = { f: mk2(20) };
  return q.f() + q.f(1);
}
console.log(inner());
