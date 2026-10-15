// A class expression written in an instance field initializer is in
// the class scope too: the constructor's parameters do not reach the
// bodies of its methods (§15.7.10).

class A {
  e = new (class {
    m() {
      return new Error("x");
    }
  })().m();
  constructor(Error: any) {}
}

class W {
  k = "outer";
}

class B {
  w = new (class {
    m() {
      return new W();
    }
  })().m();
  constructor(W: any) {}
}

function inFn() {
  class C {
    r = new (class {
      m() {
        return new RangeError("z");
      }
    })().m();
    constructor() {
      var RangeError = 5;
    }
  }
  return new C().r;
}

const a = new A(0);
console.log(a.e instanceof Error, a.e.message);
const b = new B(0);
console.log(b.w instanceof W, b.w.k);
const r = inFn();
console.log(r instanceof RangeError, r.message);
