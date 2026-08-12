// Rotation 373 (L3b 373-05) — an expression-position class inside an
// enclosing class body may extend the enclosing class: it evaluates
// when that body RUNS, after every top-level definition completed,
// so the M5.2 source-order check exempts it (ast.class_expr_deferred)
// and the field flattening resolves it in a dependency-ordered sweep.

// 1. `new (class extends Outer {})()` directly inside a method
class O3 {
  x = 1;
  mk() {
    return new (class extends O3 {})();
  }
}
const r = new O3().mk();
console.log("direct-new", r instanceof O3, (r as any).x);

// 2. static-field class expression extending the enclosing class
class O4 {
  tag() {
    return "o4";
  }
  static Sub = class extends O4 {};
}
console.log("static-field", typeof O4.Sub);

// 3. the subclass inherits fields and methods through the chain
class Base5 {
  n = 5;
  twice() {
    return this.n * 2;
  }
  mkChild() {
    return new (class extends Base5 {
      quad() {
        return this.twice() * 2;
      }
    })();
  }
}
const c = new Base5().mkChild();
console.log("inherit", c.n, c.twice(), (c as any).quad(), c instanceof Base5);
