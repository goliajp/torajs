// a class method taken as a value and called without a receiver: a body
// that never reads its receiver runs, a missing argument is undefined
class D {
  n(x: number) { return x * 10; }
  two(a: number, b: number) { return a - b; }
  static k(x: number) { return x + 1; }
}
class E extends D {}
const d = new D();
const e = new E();
const nn = d.n;
const en = e.n;
const tw = d.two;
const kk = D.k;
console.log(nn(4), nn(2.5), en(3), tw(9, 4), kk(7));
console.log((0, d.n)(4), d.n(1), d.two(5, 1));
