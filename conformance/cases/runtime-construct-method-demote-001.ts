// `new K().m()` where K holds a value, in a program where exactly one
// class owns the name `m`. The class-method rewrite is speculative —
// it fires on the NAME before any type is known — and is meant to be
// demoted once the receiver checks as something that is not that
// class. A runtime-construct receiver was not among the shapes the
// demotion would probe, so the rewrite survived and the call answered
// C's method body instead.
class C {
  m(): string {
    return "outer";
  }
}
{
  const v = "inner";
  const D: any = function () {};
  D.prototype.m = function (): string {
    return v;
  };
  console.log(new D().m());
}
console.log(new C().m());
