// Nested ClassDecl hoist — capture-free classes declared inside fn
// bodies / fn-expression bodies / arrow bodies reach the class
// machinery via the desugar_classes pre-pass hoist.

// two fns each declaring `class C` — collision α-rename axis
function f1(): number {
  class C {
    v(): number {
      return 10;
    }
  }
  return new C().v();
}
function f2(): number {
  class C {
    v(): number {
      return 20;
    }
  }
  return new C().v();
}
console.log(f1() + f2());

// extends a top-level base + instanceof through the hoisted subclass
class Base {
  b(): number {
    return 5;
  }
}
var g = function () {
  class Sub extends Base {
    s(): number {
      return this.b() + 1;
    }
  }
  const x = new Sub();
  console.log(x.s());
  console.log(x instanceof Base);
};
g();

// statics with self-reference through the class name
function h() {
  class S {
    static k: number = 42;
    static m(): number {
      return S.k;
    }
  }
  console.log(S.m());
}
h();

// arrow-body declaration
const a = () => {
  class A {
    z(): number {
      return 99;
    }
  }
  console.log(new A().z());
};
a();

// super.m() inside a nested class — the __supercall__ marker ident
// is a desugar rewrite target, not a capture
class B2 {
  m(): number {
    return 3;
  }
}
function sup(): number {
  class D extends B2 {
    m(): number {
      return super.m() + 1;
    }
  }
  return new D().m();
}
console.log(sup());

// block-nested inside a plain function, ctor + field init
function blk(): number {
  {
    class P {
      n: number = 7;
      constructor() {
        this.n = this.n + 1;
      }
    }
    return new P().n;
  }
}
console.log(blk());
