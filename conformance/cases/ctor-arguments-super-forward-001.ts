// `super(…)` fills the parent constructor's `arguments` from its own
// argument list, and the implicit constructor of a derived class is
// §15.7.10's `constructor(...args) { super(...args) }`: it forwards
// every argument the `new` site passed, past the parent's declared
// parameters too.
class P {
  n: number;
  constructor(x: number) {
    this.n = arguments.length;
  }
}
class Q extends P {
  m: number;
  constructor(a: number, b?: number) {
    super(a, 1, 2);
    this.m = arguments.length;
  }
}
const q1 = new Q(5);
const q2 = new Q(5, 6);
console.log(q1.n, q1.m, q2.n, q2.m, new P(1).n);

class B {
  n: number;
  s: string;
  constructor(x: number) {
    this.n = arguments.length;
    this.s = [...arguments].join(",");
  }
}
class R extends B {}
class S extends B {
  constructor(...args: number[]) {
    super(...args);
  }
}
console.log(new R(1, 2, 3).n, new R(1, 2, 3).s, new R(4).n);
console.log(new S(1, 2).n, new S(1, 2).s, new S(7).n);

class E {
  s: string;
  constructor(a: number, b: number = 10) {
    this.s = arguments.length + ":" + a + ":" + b + ":" + [...arguments].join(",");
  }
}
class F extends E {
  constructor(n: number) {
    if (n > 0) {
      super(n);
    } else {
      super(n, 5, 6);
    }
  }
}
console.log(new F(1).s, new F(0).s);

class G {
  s: string;
  constructor(a: number) {
    const f = () => arguments.length + "/" + arguments[1];
    this.s = f();
  }
}
class M extends G {}
class N extends M {}
console.log(new N(1, 2, 3).s, new M(4).s, new G(5, 6).s);
