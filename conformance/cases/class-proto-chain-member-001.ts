// L3b ⑧ — an any-lane struct instance member miss reads through the
// class prototype chain (§10.1.8.1 step 3): the reified method face,
// the wired constructor identity, inherited methods up the parent
// chain. A fully missing name keeps the undefined answer.
class C {
  m(a: number): number {
    return a + 1;
  }
}
class D extends C {
  own(): number {
    return 5;
  }
}
const ca: any = new C();
console.log(typeof ca.m);
console.log(ca.m);
console.log(ca.m.toString());
console.log(ca.constructor === C);
const da: any = new D();
console.log(typeof da.m);
console.log(typeof da.own);
console.log(da.constructor === D);
console.log(da.constructor === C);
console.log(da.missing);
