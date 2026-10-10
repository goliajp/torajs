// §10.4.4 CreateUnmappedArgumentsObject sizes a constructor's
// `arguments` by the argument list of the `new` expression, not by the
// declared parameters. With `new` sites of different sizes the count
// and the values have to reach the constructor at run time, through
// the class factory.
class A {
  n: number;
  constructor(x: number, y?: number) {
    this.n = arguments.length;
  }
}
console.log(new A(1).n, new A(1, 2).n);

class D {
  s: string;
  constructor(a: number) {
    const xs = [...arguments];
    this.s = arguments.length + ":" + arguments[0] + ":" + arguments[1] + ":" + xs.join(",");
  }
}
console.log(new D(1).s);
console.log(new D(1, 2, 3).s);

class C {
  n: number;
  v: string;
  constructor() {
    this.n = arguments.length;
    this.v = String(arguments[1]);
  }
}
console.log(new C().n, new C(1, 2).n, new C(1, 2).v, new C(3).v);

class E {
  s: string;
  constructor(a: number, b: number = 10) {
    this.s = arguments.length + ":" + a + ":" + b + ":" + arguments[1] + ":" + [...arguments].join(",");
  }
}
console.log(new E(1).s, new E(1, 2).s, new E(1, 2, 3).s);

class L {
  n: number;
  constructor(x: number) {
    this.n = arguments.length;
  }
}
const xs = [1, 2, 3, 4];
console.log(new L(...xs).n, new L(1).n, new D(...xs).s);

class Box<T> {
  n: number;
  v: T;
  constructor(v: T) {
    this.v = v;
    this.n = arguments.length;
  }
}
console.log(new Box<number>(1).n, new Box<string>("a").n);
