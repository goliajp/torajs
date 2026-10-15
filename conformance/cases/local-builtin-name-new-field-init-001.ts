// An instance field initializer is evaluated in the class scope, out
// of reach of the constructor's parameters and locals (§15.7.10).

class A {
  e = new Error("x");
  constructor(Error: any) {}
}

class B {
  t = new TypeError("y");
  constructor() {
    var TypeError = 5;
  }
}

function inFn() {
  class C {
    r = new RangeError("z");
    constructor(RangeError: any) {}
  }
  return new C(1).r;
}

const a = new A(1);
console.log(a.e instanceof Error, a.e.message);
const b = new B();
console.log(b.t instanceof TypeError, b.t.message);
const r = inFn();
console.log(r instanceof RangeError, r.message);
