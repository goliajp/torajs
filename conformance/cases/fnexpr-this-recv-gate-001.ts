// 398-06 — fn-expr this in concrete-signature slots: the runtime
// FLAG_CLOSURE_RECV_FIRST gate seeds strict-mode undefined on
// detached calls and the method receiver on method calls.
class A {
  f: () => void;
  n = 0;
  constructor() {
    this.f = function () { console.log("hC", typeof this); };
  }
}
const a = new A();
const gA = a.f;
gA();

class B {
  f: (x: number) => void = function (x: number) { console.log("hD", typeof this, x); };
}
const b = new B();
const gB = b.f;
gB(3);
b.f(5);

class B4 {
  f: (x: number, y: number) => void;
  constructor() {
    this.f = function (x: number, y: number) { console.log("hD4", typeof this, x, y); };
  }
}
const gB4 = new B4().f;
gB4(7, 9);

class M {
  run() {
    const g: (x: number) => string = function (x: number) { return typeof this + x; };
    return g(1);
  }
}
console.log("hG", new M().run());

const gTop: (x: number) => string = function (x: number) { return typeof this + x; };
console.log("hG5", gTop(1));

class Q {
  f: () => number;
  v = 5;
  constructor() { this.f = function () { return (this as any).v; }; }
}
const gQ = new Q().f;
try {
  gQ();
  console.log("hJ no-throw");
} catch (e) {
  console.log("hJ", e instanceof TypeError);
}
