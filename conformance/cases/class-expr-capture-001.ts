// 393-01 / RFC 20260814 blade 6 — a class EXPRESSION that reads a
// binding from the scope around it. The parser used to splice every
// class expression to the top level, where a captured local resolves
// nowhere (a warning plus a runtime ReferenceError — silent-wrong);
// now one minted outside a class body lands next to its use site, so
// the nested-class machinery decides: capture-free lifts, capturing
// takes the ES5 lane.

// block-scoped capture — the 393-01 probe itself
{
  let a = 7;
  const C = class {
    m() {
      return a;
    }
  };
  console.log(new C().m());
}

// fresh identity per enclosing call, each closed over its own env
function mk(n: any) {
  const C = class {
    m() {
      return n;
    }
  };
  return new C();
}
console.log(mk(1).m(), mk(2).m());

// named class expression, capturing
{
  let a = 3;
  const C = class Inner {
    m() {
      return a + 1;
    }
  };
  console.log(new C().m());
}

// ctor + instance field + method, alias `new` in a later statement
function withCtor(k: any) {
  const C = class {
    v: any;
    constructor() {
      this.v = k * 2;
    }
    get2() {
      return this.v;
    }
  };
  const x = new C();
  return x.get2();
}
console.log(withCtor(4));

// instanceof through the prototype link
{
  let t = 1;
  const C = class {
    m() {
      return t;
    }
  };
  const o = new C();
  console.log(o instanceof C, o.m());
}

// static method capturing (blade-2 machinery through the expr lane)
{
  let a = 5;
  const C = class {
    static s() {
      return a;
    }
  };
  console.log(C.s());
}

// captured binding reassigned after the class — methods read the cell
{
  let a = 1;
  const C = class {
    m() {
      return a;
    }
  };
  a = 42;
  console.log(new C().m());
}

// parenthesized immediate new, capturing
{
  let a = 2;
  console.log(new (class {
    m() {
      return a;
    }
  })().m());
}

// bare `new class { … }()`, capturing
{
  let a = 4;
  new (class {
    go() {
      console.log(a);
    }
  })().go();
}

// control half 1 — capture-free nested class expression still lifts
function cf() {
  const C = class {
    m() {
      return 5;
    }
  };
  return new C().m();
}
console.log(cf());

// control half 2 — capture-free nested with a static, alias dot-call
function cfs() {
  const C = class {
    static s() {
      return 9;
    }
  };
  return C.s();
}
console.log(cfs());

// control half 3 — top-level class expression keeps the old splice
const Top = class {
  m() {
    return 11;
  }
};
console.log(new Top().m());

// control half 4 — class expression inside a class body keeps the
// deferred top-level path
class A {
  m() {
    const K = class {
      n() {
        return 2;
      }
    };
    return new K().n();
  }
}
console.log(new A().m());
