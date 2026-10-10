// A spread argument list reaching a constructor whose last parameter is
// a rest parameter, at `new C(...xs)` and at `super(...xs)`, when the
// spread covers fixed parameters too: the first values bind the fixed
// parameters and the rest array collects the tail (§10.2.11), whether
// or not the constructor reads `arguments`.
const xs = [1, 2, 3, 4];

class P {
  s: number;
  constructor(a, ...r) {
    this.s = r.length;
  }
}
console.log(new P(...xs).s);
class Q extends P {
  constructor(...ys) {
    super(...ys);
  }
}
console.log(new Q(...xs).s, new Q(5).s, new Q().s);

class N {
  s: number;
  constructor(a: number, ...r: number[]) {
    this.s = a + r.length;
  }
}
console.log(new N(...xs).s);
class M extends N {
  constructor(...ys: number[]) {
    super(...ys);
  }
}
console.log(new M(7, 8).s);

class A {
  n: string;
  constructor(a, ...r) {
    this.n = arguments.length + ":" + r.length + ":" + arguments[2];
  }
}
class B extends A {
  constructor(...ys) {
    super(...ys);
  }
}
console.log(new B(...xs).n, new A(...xs).n, new A(0, ...xs).n);

function g(a, ...r) {
  return arguments[0] + ":" + arguments[2] + ":" + [...arguments].length;
}
console.log(g(...xs), g(0, ...xs));
