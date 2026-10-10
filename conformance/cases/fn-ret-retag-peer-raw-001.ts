// a vtable slot retagged to the closure lane by one row that returns
// an any value takes along rows that return a bare function value of
// another origin: a fn-typed parameter, or a const binding of a
// top-level function; each hands back that function's one fn object
const objs: any = { f: (n: number) => n + 1 };
function dbl(n: number): number { return n * 2; }
function sq(n: number): number { return n * n; }
class Base {
  pick(f: (n: number) => number): (n: number) => number { return f; }
  get(): (n: number) => number {
    const k = dbl;
    return k;
  }
}
class Sub extends Base {
  pick(f: (n: number) => number): (n: number) => number {
    const v = objs.f;
    return v;
  }
  get(): (n: number) => number {
    const v = objs.f;
    return v;
  }
}
const b: Base = new Base();
const s: Base = new Sub();
console.log(b.pick(dbl)(5), b.pick(sq)(5), s.pick(dbl)(5));
console.log(b.get()(6), s.get()(6));
console.log(b.pick(dbl) === dbl, b.pick(sq) === dbl, b.get() === dbl);
const xs: Base[] = [b, s];
let t = 0;
for (let i = 0; i < 1000; i++) for (const x of xs) t += x.pick(i % 2 ? dbl : sq)(i) + x.get()(1);
console.log(t);
