// A function expression whose `this` the method body leaves unclaimed
// still answers where the call site hands it the method's own receiver,
// a plain function's plain-call receiver, or a name another scope owns.
// Each function spells its own names: a name shared across functions
// declines the receiver promotion on its own.
function keep(_: any) {}

class C {
  v = 7;
  viaCall() {
    const h1 = function () { return this.v };
    keep(h1);
    return h1.call(this);
  }
  viaApply(...a: any[]) {
    const h2 = function (x: any) { return this.v + x };
    keep(h2);
    return h2.apply(this, a);
  }
  aliasCall() {
    const f3 = function () { return this.v * 2 };
    keep(f3);
    const g3 = f3;
    return g3.call(this);
  }
  siblingBlocks() {
    { const f4 = function () { return this }; keep(f4) }
    { const f4 = () => 1; return f4() }
  }
  reassigned() {
    let f5: any = function () { return this };
    keep(f5);
    f5 = () => 2;
    return f5();
  }
  plainCall() {
    const f6 = function () { return this === undefined };
    keep(f6);
    return f6();
  }
}

function outer() {
  const f7 = function () { return this === undefined };
  keep(f7);
  return f7();
}

const c = new C();
console.log(c.viaCall(), c.viaApply(3), c.aliasCall());
console.log(c.siblingBlocks(), c.reassigned(), c.plainCall());
console.log(outer());
