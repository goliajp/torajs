// The instance field initializers spliced into a constructor are found
// by what they are, not by the class's name: two functions may each
// declare a `class A`, and a program's own `this.a = k` in the
// constructor body is constructor code that reads the constructor's
// own bindings (§15.7.10 keeps only the initializers in the class
// scope).

const k = 1;

// a field initializer in one `class A`, a constructor assignment in
// another
function f1() {
  class A {
    x = 1;
    constructor() {}
  }
  return new A().x;
}
function g1() {
  class A {
    a: number;
    constructor(k: number) {
      this.a = k;
    }
  }
  return new A(5).a;
}
console.log("same name", f1(), g1());

// the constructor's arrow writes its own parameter
function g2() {
  class A {
    h: () => number;
    constructor(k: number) {
      this.h = () => {
        k = 2;
        return k;
      };
    }
  }
  return new A(5).h();
}
console.log("ctor arrow", f1(), g2());

// a class with no constructor of its own next to one whose
// constructor assigns
function f3() {
  class A {
    x = 1;
  }
  return new A().x;
}
function g3() {
  class A {
    a: number;
    constructor(k: number) {
      this.a = k;
    }
  }
  return new A(7).a;
}
console.log("synth ctor", f3(), g3());

// initializers followed by the constructor's own assignment in one
// class: the field reads the outer `k`, the assignment the parameter
class M {
  v = k;
  w: number;
  constructor(k: number) {
    this.w = k;
  }
}
const m = new M(9);
console.log("mixed", m.v, m.w);

// a derived class whose constructor runs other statements before
// `super()`, some of them rewritten into more statements
class Base {
  b = 10;
}
class Derived extends Base {
  v = k;
  w: number;
  constructor(k: number) {
    const r = eval("k + 1");
    let s = 0;
    for (const i of [1, 2]) s += i;
    super();
    this.w = k + r + s;
  }
}
const dv = new Derived(4);
console.log("derived", dv.b, dv.v, dv.w);

// a private field next to a public one and a constructor assignment
class P {
  #p = k + 1;
  q = k + 2;
  r: number;
  constructor(k: number) {
    this.r = k;
  }
  get p() {
    return this.#p;
  }
}
const p = new P(20);
console.log("private", p.p, p.q, p.r);
