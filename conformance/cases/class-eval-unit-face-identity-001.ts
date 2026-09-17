// §15.7.14 — each evaluation of a class definition produces its own
// method function objects, so two evaluations' `C.prototype.m` are
// distinct objects (RFC 20260917-class-eval-unit K2b).
function outer() {
  class C {
    v = 1;
    plain() { return this.v }
  }
  return C;
}
const C1 = outer(), C2 = outer();
console.log(C1.prototype.plain === C2.prototype.plain);
console.log(C1.prototype.plain === C1.prototype.plain);
console.log(typeof C1.prototype.plain);
const c1: any = new C1();
console.log(C1.prototype.plain.call(c1));

// a loop body evaluates the class once per iteration
const ks: any[] = [];
for (let i = 0; i < 3; i++) {
  class K { m() { return 7 } }
  ks.push(K.prototype.m);
}
console.log(ks[0] === ks[1], ks[1] === ks[2]);
console.log(ks[0].call(null), ks[2].call(null));
