// An instance field initializer runs in the class body's scope
// (§15.7.10 ClassFieldDefinitionEvaluation), not the constructor's:
// a constructor parameter, a destructured parameter, a local
// `let` / `const` / `var` or a function declaration of the same name
// does not reach it, and the constructor still reads its own binding.
// A function nested in the constructor that rebinds the name keeps
// its own.

// a parameter
const k = 1;
class E {
  v = k;
  w: number;
  constructor(k: number) {
    this.w = k;
  }
}
const e = new E(5);
console.log("param", e.v, e.w);

// a local `const`, written to from a field arrow
let lk = 1;
class D {
  f = () => {
    lk = 2;
  };
  seen: number;
  constructor() {
    const lk = 7;
    this.seen = lk;
  }
}
const d = new D();
d.f();
console.log("local", lk, d.seen);

// no name in common
const a = 3;
class N {
  x = a + 1;
  y: number;
  constructor(b: number) {
    this.y = b;
  }
}
const n = new N(4);
console.log("plain", n.x, n.y);

// a destructured parameter
const q = "outer";
class Q {
  v = q;
  w: string;
  constructor({ q }: { q: string }) {
    this.w = q;
  }
}
const qq = new Q({ q: "inner" });
console.log("destructured", qq.v, qq.w);

// a derived class: the initializers run after `super()`
const z = 10;
class B0 {
  base = 1;
}
class Dz extends B0 {
  v = z;
  u: number;
  constructor(z: number) {
    super();
    this.u = z;
  }
}
const dz = new Dz(20);
console.log("derived", dz.base, dz.v, dz.u);

// a nested function rebinding the name keeps its own binding
const s = "s-outer";
class S {
  v = s;
  r: string;
  constructor(s: string) {
    const g = (s: string) => s + "!";
    this.r = g("x") + s;
  }
}
const ss = new S("arg");
console.log("nested", ss.v, ss.r);

// a `var`
const vv = "o";
class V {
  v = vv;
  w: string;
  constructor() {
    var vv = "i";
    this.w = vv;
  }
}
const v = new V();
console.log("var", v.v, v.w);

// a function declaration
function fd() {
  return "outer";
}
class F {
  v = fd();
  w: string;
  constructor() {
    this.w = fd();
    function fd() {
      return "inner";
    }
  }
}
const f = new F();
console.log("function", f.v, f.w);

// a parameter another parameter's default reads
const pd = 1;
class P {
  v = pd;
  p: number;
  e: number;
  constructor(pd = 2, e = pd + 1) {
    this.p = pd;
    this.e = e;
  }
}
const p = new P();
console.log("default", p.v, p.p, p.e);

// a field arrow writing the outer binding the parameter shadows
let wk = 0;
class W {
  set = (x: number) => {
    wk = x;
  };
  constructor(wk: number) {
    this.set(wk);
  }
}
new W(9);
console.log("write", wk);
