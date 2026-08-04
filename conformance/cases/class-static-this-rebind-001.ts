// RFC 20260804-fn-this-channel knife 3 — a static method's this
// rebinds through .call/.apply: the __smany_ twin reads the receiver
// through the any lane (private brand check included).
class C {
  v = 7;
  static g() {
    return (this as any).v;
  }
}
console.log(C.g.call(new C()));
console.log(C.g.apply(new C(), []));

class D {
  w = 3;
  static h(n: number) {
    return (this as any).w + n;
  }
}
console.log(D.h.call(new D(), 10));
console.log(D.h.call({ w: 100 }, 1));
