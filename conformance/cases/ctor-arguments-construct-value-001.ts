// A class constructed through a value (`new (C as any)(…)`,
// `Reflect.construct`) enters its factory's boxed adapter, which holds
// the true argument list; the constructor's `arguments` describes that
// list, whatever the direct `new` sites in the program look like.
class A {
  n: number;
  constructor(x: number, y?: number) {
    this.n = arguments.length;
  }
}
const AA: any = A;
console.log(new AA(5).n, new AA().n, new AA(1, 2, 3).n);
console.log(Reflect.construct(A, [1, 2, 3, 4]).n, Reflect.construct(A, []).n);
console.log(new A(9).n);

class V {
  n: number;
  s: string;
  constructor(x: number) {
    this.n = arguments.length;
    this.s = String(arguments[1]);
  }
}
const VV: any = V;
console.log(new VV(5).n, new VV().n, new VV(1, 2, 3).n, new VV(1, 2, 3).s);
console.log(Reflect.construct(V, [1, 2, 3, 4]).n, Reflect.construct(V, []).n);
console.log(new V(9).n, new V(9, 8).s);

class E {
  s: string;
  constructor(a: number, b: number = 10) {
    this.s = arguments.length + ":" + a + ":" + b + ":" + arguments[1] + ":" + [...arguments].join(",");
  }
}
const EE: any = E;
console.log(new EE().s, new EE(4).s, Reflect.construct(E, [7, 8, 9]).s, new EE(true).s);

class U {
  s: string;
  constructor(a: number) {
    this.s = arguments.length + ":" + a + ":" + arguments[0];
  }
}
const UU: any = U;
console.log(new UU(true).s, new U(1).s, new U(1, 2).s);

class G {
  s: string;
  constructor(a: number) {
    this.s = arguments.length + "/" + arguments[1];
  }
}
class M extends G {}
class N extends M {}
const NN: any = N;
console.log(new NN(7, 8).s, new N(1, 2, 3).s);
