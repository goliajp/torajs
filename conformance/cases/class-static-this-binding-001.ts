// 420-03 — `this` inside a static field initializer or a static block
// is the class object (ES §15.7.14 runs both with the class as
// receiver). The parser only recorded the binding for static METHOD
// bodies, so these two positions fell through to the instance receiver:
// `static b = this.a` answered NaN, `static { this.x = 1 }` threw, and
// a `this.m()` call threw "value is not a function".
let out: string[] = [];
class C {
  static a = 1;
  static b = (this as any).a + 1;
  static m(): number { return 7 }
  static v = (this as any).m();
  static { out.push("blk:" + (this as any).b); (this as any).z = 9; }
  static w = (this as any).z + 1;
}
console.log(C.a, C.b, C.v, (C as any).z, (C as any).w, out.join(","));
console.log(typeof (C as any), (C as any) === C);

// Identity: the receiver IS the class binding, in both positions.
class D {
  static self = (this as any);
  static { out.push("same:" + ((this as any) === D)); }
}
console.log((D as any).self === D, out.join(","));

// Same class one function level down, where the in-place lane owns the
// lowering — its emit hands the body the class object as an ordinary
// function receiver, so the registration must be dropped there.
function make(): string {
  class E {
    static a = 2;
    static b = (this as any).a * 3;
    static { (this as any).c = "c" }
  }
  return String(E.b) + (E as any).c;
}
console.log(make(), make());

// An instance field initializer keeps the instance receiver.
class F {
  n = 4;
  m = (this as any).n + 1;
}
console.log(new F().m);
