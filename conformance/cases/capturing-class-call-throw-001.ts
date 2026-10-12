// A class constructor has no [[Call]]: calling it through any channel
// throws a TypeError, whether or not the class reads its enclosing scope.
function attempt(label: string, f: () => any) {
  try {
    f();
    console.log(label, "no throw");
  } catch (e) {
    console.log(label, e instanceof TypeError);
  }
}
function capturing() {
  const table: any = { a: "A" };
  class X {
    constructor(k: any) {
      return table[k];
    }
  }
  class Y extends X {}
  attempt("call", () => (X as any)("a"));
  attempt("sort", () => [2, 1].sort(X as any));
  attempt("replace", () => "a".replace(/a/, X as any));
  attempt("dot-call", () => (X as any).call({}, "a"));
  attempt("apply", () => (X as any).apply({}, ["a"]));
  attempt("derived", () => (Y as any)());
  console.log(typeof new X("a"), new Y("a") instanceof X);
}
capturing();
function nonCapturing() {
  class Z {
    constructor(x: any, y: any) {}
  }
  attempt("nc-sort", () => [1, 2, 3].sort(Z as any));
  attempt("nc-call", () => (Z as any)());
  console.log(new Z(1, 2) instanceof Z);
}
nonCapturing();
class Top {
  constructor(x: any, y: any) {}
}
attempt("top-sort", () => [2, 1].sort(Top as any));
attempt("top-replace", () => "a".replace(/a/, Top as any));
