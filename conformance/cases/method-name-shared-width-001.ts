// A class method and an object literal's own method of the same name:
// a call on the object reaches the object's method with the arguments
// as written, whatever the class's method takes. Methods named `call`
// / `apply` on an object or a class instance are their own methods,
// not Function.prototype's.
class Caller {
  base = 1;
  calc(a: number, b: number) {
    return this.base + a + b;
  }
  pick(xs: number[]) {
    return xs[0];
  }
  call(a: number, b: number) {
    return a - b;
  }
  apply(a: number, xs: number[]) {
    return a + xs.length;
  }
  mk(n: number): number[] {
    return [n, n + 1];
  }
}
const c = new Caller();
console.log(c.calc(1, 2), c.pick([3]), c.call(5, 1), c.apply(2, [1, 2]), c.mk(1)[1]);
const o = {
  k: 2,
  calc(t: number, x: number) {
    return t * x;
  },
  pick(xs: number[]) {
    return xs[0] / 2;
  },
  call(t: number, x: number) {
    return t * x;
  },
  apply(t: number, xs: number[]) {
    return t * xs[0];
  },
  mk(n: number): number[] {
    return [n / 2, n * 1.5];
  },
};
let r = o.calc(3, 1.5);
const q = o.pick([3]);
console.log(r, o.calc(0.5, 4), q, o.pick([1.5]));
console.log(o.call(3, 1.5), o.call(0.5, 4), o.apply(0.5, [3]), c.call(1, 0.25));
const ys = o.mk(3);
console.log(ys[0], ys[1], o.mk(1)[0]);
let acc = 0;
for (let i = 0; i < 3; i++) acc += o.calc(i, 0.25);
console.log(acc);
