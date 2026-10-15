// A function expression whose `this` the enclosing body leaves
// unclaimed still compiles where its value cannot reach a receiver
// other than the one its body reads: an argument nobody reads, a store
// onto the enclosing receiver read back through it, `bind(this)`, and
// reads that never invoke it.
function keep(_: any) {}

class C {
  v = 7;
  h: any;
  inert() {
    const f1 = function () { return this.v };
    keep(f1);
    return f1.call(this);
  }
  ownSlot() {
    const f2 = function () { return this.v + 1 };
    keep(f2);
    this.h = f2;
    return this.h();
  }
  boundToThis() {
    const f3 = function () { return this.v * 3 };
    keep(f3);
    return f3.bind(this)();
  }
  reads() {
    const f4 = function () { return this };
    keep(f4);
    return [typeof f4, f4 === f4, !f4].join(" ");
  }
}

function It(this: any) {
  this.v = 4;
  this.get = function () { return this.v };
  this[Symbol.iterator] = function () { return this };
  keep(this.get);
}

const c = new C();
console.log(c.inert(), c.ownSlot(), c.boundToThis());
console.log(c.reads());
const it = new (It as any)();
console.log(it.get(), it[Symbol.iterator]() === it);
