// §10.2.1.2 — an ordinary function binds its own `this`. Inside a
// static member body the parser records "this means the class object",
// and that recording has to stop at a nested `function`: the one
// written there never receives the class as a receiver. It did not
// stop, and both spellings died on a name minted for a receiver they
// never had (`closure capture __class_C not in scope`). An arrow is
// the exception — it has no `this` of its own and keeps reading the
// class.
class Counter {
  static viaExpr() {
    return (function () {
      return this;
    })();
  }
  static viaDecl() {
    function g(this: any) {
      return typeof this;
    }
    return g();
  }
  static direct() {
    return this === Counter;
  }
}

console.log(Counter.viaExpr());
console.log(Counter.viaDecl());
console.log(Counter.direct());
