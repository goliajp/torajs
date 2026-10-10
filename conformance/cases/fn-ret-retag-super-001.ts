// `super.get()` calls the base row of a vtable slot retagged to the
// closure lane, and gets back the fn object of the named function
// that row returns
const objs: any = { f: (n: number) => n + 1 };
function dbl(n: number): number { return n * 2; }
class Base {
  get(): (n: number) => number { return dbl; }
}
class Sub extends Base {
  get(): (n: number) => number {
    const v = objs.f;
    return v;
  }
  base(): (n: number) => number { return super.get(); }
}
const s = new Sub();
console.log(s.base()(3), s.get()(3), s.base() === dbl, new Base().get()(4));
let t = 0;
for (let i = 0; i < 1000; i++) t += s.base()(i);
console.log(t);
