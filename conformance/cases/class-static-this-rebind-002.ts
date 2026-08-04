// RFC 20260804-fn-this-channel knife 3d — a static body that CALLS a
// method on its receiver. The call form is a speculative-demote alt
// living off the side table, so the twin mint has to rebind the
// receiver a second time (after the alt subtree is deep-cloned in);
// otherwise the twin reads the class object and the call misses.
class C {
  #f() {
    return 42;
  }
  static g() {
    return (this as any).#f();
  }
}
console.log(C.g.call(new C()));
try {
  C.g();
  console.log("no throw");
} catch (e) {
  console.log((e as any).constructor.name);
}

class D {
  n = 5;
  pub() {
    return this.n * 2;
  }
  static k(extra: number) {
    return (this as any).pub() + extra;
  }
}
console.log(D.k.call(new D(), 1));
console.log(D.k.apply(new D(), [100]));

// receiver reads and receiver calls in one body
class E {
  base = 4;
  scale() {
    return this.base * 3;
  }
  static both() {
    const self = this as any;
    return self.base + self.scale();
  }
}
console.log(E.both.call(new E()));
