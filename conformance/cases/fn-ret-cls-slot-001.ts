// a vtable slot whose rows hand back one fn type, some as the bare
// function and some as its fn object: every row hands back the fn
// object, including a row returning its own fn-typed param
type F = (n: number) => number;
const objs: any = { f: (n: number) => n + 1 };
function dbl(n: number): number { return n * 2; }
class A {
  get(k: number): F { return dbl; }
  pick(f: F): F { return f; }
}
class B extends A {
  get(k: number): F { return (n) => n + k; }
  pick(f: F): F { return objs.f; }
}
const xs: A[] = [new A(), new B()];
for (const x of xs) console.log(x.get(3)(5), x.pick(dbl)(5));
console.log(xs[0].get(1) === dbl, xs[0].pick(dbl) === dbl);
