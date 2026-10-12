// a capturing subclass's super(…) constructs its capturing sibling parent with its own new.target
function make(tag: string) {
  const seen: string[] = [];
  class A {
    a: number;
    constructor(x: number, y: number = x + 1) {
      seen.push(tag + ":" + (new.target === B) + ":" + (new.target === C) + ":" + y);
      this.a = x + y;
    }
  }
  class B extends A {
    b: number;
    constructor(x: number) {
      super(x);
      this.b = x * 2;
    }
  }
  class M extends B {}
  class C extends M {
    c = 3;
  }
  class D extends A {
    constructor(...xs: number[]) {
      super(xs[0], xs[1]);
    }
  }
  return { A, B, C, D, M, seen };
}
const { A, B, C, D, M, seen } = make("t");
const b = new B(4);
console.log(b.a, b.b, b instanceof A, b instanceof B);
const c = new C(1);
console.log(c.a, c.b, c.c, c instanceof M, c instanceof A);
const d = new D(5, 6);
console.log(d.a, d instanceof D);
const r: any = Reflect.construct(B, [2], C);
console.log(r.a, r.b, Object.getPrototypeOf(r) === C.prototype);
const BB: any = B;
try {
  BB(1);
} catch (e: any) {
  console.log(e instanceof TypeError);
}
console.log(seen.join(" "));
