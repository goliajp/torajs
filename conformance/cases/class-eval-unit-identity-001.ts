// RFC 20260917-class-eval-unit K1 — a class declared in a function or
// loop body is a new class object and prototype on every evaluation.
function outer() {
  class C {
    v: number;
    constructor(v: number) { this.v = v; }
    get2(): number { return this.v * 2; }
  }
  return C;
}
const C1: any = outer();
const C2: any = outer();
console.log(C1 === C2, typeof C1, C1.name, C1.length);
function make(v: number): any {
  class K { x: number; constructor(x: number) { this.x = x; } twice(): number { return this.x + this.x; } }
  const k = new K(v);
  console.log(k instanceof K, Object.getPrototypeOf(k) === K.prototype, k.twice(), k.constructor === K);
  return [K, k];
}
const [K1, k1] = make(1);
const [K2, k2] = make(2);
console.log(K1 === K2, k1 instanceof K2, k2 instanceof K1, k1 instanceof K1);
console.log(Object.getPrototypeOf(k1) === K1.prototype, Object.getPrototypeOf(k2) === K2.prototype);
console.log(Object.keys(k1), JSON.stringify(k2), k1);
const ks: any[] = [];
for (let i = 0; i < 3; i++) { class L { n = i; } ks.push(L); }
console.log(ks[0] === ks[1], ks[1] === ks[2]);
function outer2() { class CC { m() { return 1; } } return CC; }
const D1: any = outer2();
console.log(D1);
console.log(String(D1).slice(0, 7));
try { const o = new D1(); console.log(o.m(), Object.getPrototypeOf(o) === D1.prototype); } catch (e) { console.log("new-value", (e as any).message); }
console.log(Object.getOwnPropertyNames(D1.prototype), typeof D1.prototype.m);
