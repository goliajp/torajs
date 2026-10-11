// a parameter default is evaluated in the function's own scope
// (ECMA-262 §10.2.11), so it reads what the function's definition
// scope binds — here a nested class whose member defaults read the
// enclosing function's locals
class A {}
const g = (k: number) => {
  class C extends A {
    constructor() {
      super();
      const h = (y: number = k) => y;
      console.log("after super:", h(), h(3));
    }
  }
  return new C();
};
g(7);

function plain(k: number) {
  class C {
    constructor() {
      const h = (y: number = k) => y;
      console.log("plain ctor:", h(), h(3));
    }
    m() {
      const h = (y: number = k + 1) => y;
      return h();
    }
  }
  return new C().m();
}
console.log("arrow in method:", plain(7));

function methodDefault(k: number) {
  class C {
    m(y: number = k) {
      return y;
    }
  }
  return new C();
}
const c = methodDefault(4);
console.log("method default, called outside:", c.m(), c.m(1));

function ctorDefault(k: string) {
  class D {
    s: string;
    constructor(s: string = k + "!") {
      this.s = s;
    }
  }
  return D;
}
const D = ctorDefault("q");
console.log("ctor default, constructed outside:", new D().s, new D("z").s);

function nestedClass(k: number) {
  class D {
    m(y: number = k) {
      return y;
    }
  }
  const d = new D();
  {
    const k = 77;
    return [d.m(), k].join(",");
  }
}
console.log("nested class:", nestedClass(6));
