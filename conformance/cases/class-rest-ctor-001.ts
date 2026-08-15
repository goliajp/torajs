// A class ctor's rest param carries the true call-site argc through
// the factory relay (the double-wrap regression: `new C(1,2,3)` used
// to answer args.length 1), through an explicit `super(...a)`
// forward, and through the derived default ctor (which used to skip
// rest-tailed ancestors outright, silently dropping the super call).
class C {
  n: number;
  constructor(...args: any[]) {
    this.n = args.length;
  }
}
console.log(new C().n, new C(1).n, new C(1, 2, 3).n);
class D extends C {
  constructor(...a: any[]) {
    super(...a);
  }
}
console.log(new D().n, new D(1, 2).n);
class E extends C {}
console.log(new E(7, 8).n);
class T {
  s: number;
  constructor(...xs: number[]) {
    let t = 0;
    for (const x of xs) t = t + x;
    this.s = t;
  }
}
console.log(new T(1, 2, 3).s, new T().s);
