// 420-04 — `super.m()` inside a static field initializer or a static
// block. Both are static member bodies (§15.7.14 runs them with the
// class as receiver), so their home object is the class and the super
// base is the parent CLASS — the same rewrite a static METHOD body
// already got. Only the method list was walked, so these two kept the
// parser's raw `__supercall__<m>` marker and died at typecheck with
// "unknown identifier `__supercall__bm`".
class B {
  static bs = 1;
  static bm(): number { return 2 }
  static tag(): string { return "B" }
}
class D extends B {
  static a = super.bm() + 100;
  static { console.log("blk", super.bm(), super.tag()); }
  static m(): number { return super.bm() + 1 }
}
console.log((D as any).a, D.m(), (D as any).bs);

// The nearest declaring ancestor answers, not the direct parent.
class M extends D {
  static z = super.tag() + "!";
}
console.log((M as any).z);

// A class inside a function takes the in-place lane; it lowers the
// same call against the parent binding.
function make(): string {
  class E extends B {
    static e = super.tag() + "-e";
  }
  return (E as any).e;
}
console.log(make(), make());
