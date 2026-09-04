// A class-method call whose argument count is only known at runtime.
// The name-based `__cm_` rewrite cannot spell one, and neither static
// expander takes these four shapes, so the call goes back to the
// member form the rewrite replaced and dispatches through the runtime
// spread lane.
class C {
  two(a: any, b: any) {
    console.log("two", a, b);
  }
  four(a: any, b: any, c: any, d: any) {
    console.log("four", a, b, c, d);
  }
  one(a: any) {
    console.log("one", a);
  }
}

const c = new C();
const xs = [1, 2];
const ys = [3, 4];

// two spreads
c.four(...xs, ...ys);
// a spread that is not last
c.four(...xs, 9, 10);
// a source that is not a plain name
c.one(...[7].map((n) => n));
// a fixed prefix longer than the declared arity
c.one(1, 2, 3, ...xs);
// the receiver is an expression, and the method is inherited
class D extends C {}
new D().four(...ys, ...xs);
// a method reached through the prototype
C.prototype.two.call(c, ...xs);
