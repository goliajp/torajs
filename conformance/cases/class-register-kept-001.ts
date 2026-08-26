// RFC 20260824-s2-5 刀 4 A8 — the shapes that must KEEP the class
// register call and the method rows' twins: a registry reader is
// live (instance `.constructor`, `Object.getPrototypeOf`,
// `instanceof` through any), or the class cell itself is read
// (`A.prototype`, `typeof A`, the cell handed to an any binding), or
// a prototype face is invoked with a re-bound receiver (the twin's
// only job). A stripped register would answer `undefined` / `false`
// here — never silently.
class A {
  x = 40;
  get2() {
    return this.x + 2;
  }
}
const a = new A();
console.log(a.constructor === A, Object.getPrototypeOf(a) === A.prototype);
console.log(typeof A, A.prototype.constructor === A);
const K: any = A;
const k = new K();
console.log(k instanceof A, k.get2());
const other = { x: 100 };
console.log(A.prototype.get2.call(other));
const fn = a.get2;
console.log(typeof fn);
