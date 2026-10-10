// a fn-typed return the checker types any, reached through the
// function's own fn object and through a vtable slot
const objs: any = { f: (n: number) => n + 1, g: (n: number) => n * 10 };
function pick(k: number): (n: number) => number {
  const v = k > 0 ? objs.f : objs.g;
  return v;
}
const g: any = pick;
console.log(g(1)(4), g(0)(4));
const h = pick;
console.log(h(1)(2), pick(0)(2), h === pick);
const arr: any[] = [pick];
console.log(arr[0](1)(7));

function dbl(n: number): number { return n * 2; }
class Base {
  get(): (n: number) => number { return dbl; }
}
class Sub extends Base {
  get(): (n: number) => number {
    const v = objs.f;
    return v;
  }
}
const xs: Base[] = [new Base(), new Sub()];
for (const x of xs) console.log(x.get()(10));
const b: Base = new Sub();
console.log(b.get()(3), new Base().get() === dbl);
