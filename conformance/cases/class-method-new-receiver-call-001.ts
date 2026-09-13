// `new C().k(3)` inside a method that reads `this` died in lowering
// with "unsupported member call shape: k". Such a method gets a
// `__cmany_` twin (its `this` retyped `any` for the fn-this channel);
// the twin restores every static `__cm_` call back to member shape
// so a rebound receiver dispatches dynamically, and it kept the
// static form only for a bare free name. A fresh instance's class is
// fixed by the factory pass 2 spelled it as (`__new_C`), whatever the
// twin retypes, so it keeps static dispatch too. A generator's `next`
// always reads `this`, so the same call inside any generator body —
// or any async body moved into one — hit the same wall.
class C {
  k(n: number): number {
    return n + 1;
  }
  async m(n: number): Promise<number> {
    return n * 2.5;
  }
}
class D {
  x = 1;
  f(): number {
    return this.x + new C().k(3);
  }
}
console.log("method", new D().f());
function* g(): Generator<number> {
  console.log("gen-inner", new C().k(3));
  const v = yield new C().k(10);
  console.log("gen-sent", v);
}
const it = g();
console.log("gen-step", it.next().value);
it.next(9);
(async () => {
  const u = await new C().m(4);
  console.log("async-value", u);
})();
