// RFC 20260717-objlit-anylane-recv knife 2b — a direct-call ObjectLit
// arg into an explicitly `any`-annotated param joins the any-lane
// predicate (the SSA any-param route already lowered it through the
// dynobj lane; the missing AST-side mark left this-using members with
// the nominal stamp, so the dynobj-init guard rejected the program).

function take(o: any) {
  console.log(o.f());
  console.log(o.g);
}
take({ v: 7, f() { return this.v; }, get g() { return this.v * 3; } });
// 7
// 21

// setter + get/set pair through the same route
function poke(t: any) {
  t.x = 21;
  console.log(t._x, t.x);
}
poke({
  _x: 0,
  set x(nv) {
    this._x = nv * 2;
  },
  get x() {
    return this._x + 1;
  },
});
// 42 43

// second any param position
function pair(tag: any, o: any) {
  console.log(tag, o.m());
}
pair("t", { n: 5, m() { return this.n * 10; } });
// t 50

// mixed params: only the any position promotes
function mixed(k: number, o: any) {
  console.log(k, o.m());
}
mixed(2, { n: 4, m() { return this.n + 1; } });
// 2 5

// data-only literal through an any param does not regress
function data(d: any) {
  console.log(d.a, d.b);
}
data({ a: 1, b: "two" });
// 1 two
console.log("done");
