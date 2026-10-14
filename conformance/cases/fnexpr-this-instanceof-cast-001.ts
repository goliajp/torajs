// `x instanceof (F as any)` reads F the way the bare `x instanceof F`
// does: the cast is type-only and neither spelling calls F, so it must
// not cost F the receiver it gets under `new` (§10.2.2).
class C {
  tag = 0;
  m(): void {
    const F = function (this: any) { this.tag = 5; };
    const o: any = new (F as any)();
    console.log("method", o.tag, o instanceof (F as any), this.tag);
  }
}
new C().m();
function outer(): void {
  const F = function (this: any) { this.tag = 3; };
  const o: any = new (F as any)();
  console.log("function", o.tag, o instanceof (F as any), {} instanceof (F as any));
}
outer();
