// 398-01 — method-level type parameters: `pair<T>(v: T)` on both
// static and instance members. The parser used to stop at the `<`
// ("expected `(` (method) or `:` (field)"); now the list parses with
// the standalone-fn machinery and lands on the desugared `__cm_` /
// `__sm_` FnDecl after the class-level list, so monomorphization
// serves methods with no new machinery.

class B {
  static pair<T>(v: T): any {
    return [v, v];
  }
  echo<T>(v: T): any {
    return [v, v];
  }
}
console.log(B.pair(3), B.pair("a"));
console.log(new B().echo(4), new B().echo("b"));

// T in return position and inside a param constructor
class C {
  id<T>(v: T): T {
    return v;
  }
  first<T>(xs: T[]): T {
    return xs[0];
  }
}
console.log(new C().id(5) + 1, new C().id("x") + "y");
console.log(new C().first([7, 8]), new C().first(["p", "q"]));

// method-level list concatenates after the class-level one
class G<K> {
  hold: K;
  constructor(k: K) {
    this.hold = k;
  }
  wrap<T>(t: T): any {
    return [this.hold, t];
  }
}
console.log(new G(1).wrap("z"));
