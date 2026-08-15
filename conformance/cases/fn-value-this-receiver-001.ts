// Rotation 410 — a this-writing named fn carried into an `: any`
// binding keeps its receiver channel: the binding's init takes the
// `__fwdrecv_` receiver-first forwarder, and every any-lane consumer
// (`.call`, bare call, `new`, value-shaped-parent `super`) threads
// `this` through FLAG_CLOSURE_RECV_FIRST.
function F(this: any, p: number) {
  this.v = p;
  this.m = function (this: any) {
    return this.v * 2;
  };
}

// direct .call — the static channel (pre-existing, regression lock)
const o1: any = {};
F.call(o1, 5);
console.log(o1.v);

// .call through the any binding
const B: any = F;
const o2: any = {};
B.call(o2, 7);
console.log(o2.v);

// construct through the any binding
const n: any = new B(9);
console.log(n.v, n.m(), n instanceof B);

// bare call — §10.2.1.2 strict this = undefined, the write throws
let threw = false;
try {
  B(1);
} catch (e) {
  threw = true;
}
console.log(threw);

// value-shaped parent: extends the any binding
class K extends B {
  constructor() {
    super(21);
  }
}
const k: any = new K();
console.log(k.v, k.m());

// value-shaped parent: extends the fn directly through `as any`
class K2 extends (F as any) {
  constructor() {
    super(3);
  }
}
const k2: any = new K2();
console.log(k2.v, k2.m());
