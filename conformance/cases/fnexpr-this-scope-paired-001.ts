// 399-03 — scope-paired promotion: several scopes each declare their
// own `const g = function () { …this… }`. The census pairs every use
// to its own scope's binding and promotes all groups together, so a
// direct call answers the §10.2.1.2 call-site `this` (undefined) in
// every scope — the by-name refusal used to reject them all, and the
// scope with no enclosing `__this` turned into a loud reject.

function s1() {
  const g = function () {
    return typeof this;
  };
  return g();
}
function s2() {
  const g = function (n: number) {
    return typeof this + ":" + n;
  };
  return g(7);
}
// A method-scope declaration clones into the `__cmany_` twin body —
// the twin's copy is its own group, proven and patched alongside the
// mono source (the any-lane call below runs the clone).
class D {
  x = 3;
  m() {
    const g = function () {
      return typeof this;
    };
    return g() + ":" + this.x;
  }
}
{
  const g = function () {
    return typeof this;
  };
  console.log("block", g());
}
const g = function () {
  return typeof this;
};
console.log("top", g());
const d: any = new D();
console.log(s1(), s2(), new D().m(), d.m());
